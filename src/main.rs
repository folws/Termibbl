use actix::prelude::*;
use argh::FromArgs;
use log::info;
use server::{GameOpts, GameServer};

use std::io::{stdout, Write};

use crossterm::{
    event::{read, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, MouseEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    Result,
};

use tui::{backend::CrosstermBackend, Terminal};

use client::app::ServerSession;
use data::Username;

mod client;
mod data;
mod message;
mod network;
mod server;

#[derive(FromArgs)]
/// A Skribbl.io-alike for the terminal
struct Opt {
    #[argh(subcommand)]
    cmd: SubOpt,
}

#[derive(FromArgs)]
#[argh(subcommand)]
enum SubOpt {
    Server(server::CliOpts),
    Client(client::CliOpts),
    Test(TestOpts),
}

#[derive(FromArgs)]
/// test
#[argh(subcommand, name = "test")]
pub struct TestOpts {}

#[derive(actix::Message)]
#[rtype(result = "()")]
struct StopSignal;

fn display_public_ip(port: u32) {
    tokio::spawn(async move {
        if let Ok(res) = reqwest::get("http://ifconfig.me").await {
            if let Ok(ip) = res.text().await {
                println!("Your public IP is {}:{}", ip, port);
                info!("You can find out your private IP by running \"ip addr\" in the terminal");
            }
        }
    });
}

#[actix_rt::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let cli: Opt = argh::from_env();
    match cli.cmd {
        SubOpt::Test(_) => {
            println!(
                "{:?}",
                bincode::serialize(&network::ClientMsg::Chat(network::ChatMessage::SystemMsg(
                    "something".to_owned()
                )))
            );
            // let mut stream = tokio::net::TcpStream::connect("127.0.0.1:9001").await?;
            // println!("created stream");

            // let result = stream.write(b"hello world\n").await;
            // println!("wrote to stream; success={:?}", result.is_ok());
        }
        SubOpt::Client(opt) => {
            let addr = opt.addr;
            let addr = if addr.starts_with("ws://") || addr.starts_with("wss://") {
                addr
            } else {
                format!("ws://{}", addr)
            };
            run_client(&addr, opt.username.into()).await.unwrap();
        }

        SubOpt::Server(opt) => {
            let port = opt.port;

            // display public ip
            if opt.display_public_ip {
                display_public_ip(port);
            }

            let default_game_opts: GameOpts = opt.into();
            let addr = format!("127.0.0.1:{}", port);

            // start tcp listener :: TODO: maybe use udp instead?
            let server_listener = server::listen(&addr).await;

            // start game server
            let game_server = GameServer::start(server_listener, default_game_opts);

            println!("🚀 Running Termibbl server on {}...", addr);

            tokio::signal::ctrl_c().await.unwrap();
            println!("Ctrl-C received. Stopping..");

            // gracefully exit
            game_server.do_send(StopSignal);
        }
    }

    System::current().stop();
    Ok(())
}

pub enum ClientEvent {
    MouseInput(MouseEvent),
    KeyInput(KeyEvent),
    ServerMessage(message::ToClientMsg),
}

async fn run_client(addr: &str, username: Username) -> client::error::Result<()> {
    let (mut client_evt_send, client_evt_recv) = tokio::sync::mpsc::channel::<ClientEvent>(1);

    let mut app =
        ServerSession::establish_connection(addr, username, client_evt_send.clone()).await?;

    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    execute!(stdout(), EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;
    tokio::spawn(async move {
        app.run(&mut terminal, client_evt_recv).await.unwrap();
    });
    loop {
        match read()? {
            Event::Key(evt) => match evt {
                KeyEvent {
                    code: KeyCode::Esc,
                    modifiers: _,
                } => break,
                _ => {
                    let _ = client_evt_send.send(ClientEvent::KeyInput(evt)).await;
                }
            },
            Event::Mouse(evt) => {
                let _ = client_evt_send.send(ClientEvent::MouseInput(evt)).await;
            }
            _ => {}
        }
    }

    execute!(stdout(), DisableMouseCapture)?;
    execute!(stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}
