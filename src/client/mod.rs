pub use crate::*;
pub mod app;
pub mod error;
pub mod ui;

use argh::FromArgs;

use crossterm::{
    event::{read, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, MouseEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    Result,
};

use tui::{backend::CrosstermBackend, Terminal};

#[derive(FromArgs)]
/// play Skribbl.io-like games in the Termibbl
#[argh(subcommand, name  "client")]
pub struct CliOpts {
    #[argh(positional)]
    ///username to connect as.
    username: String,

    #[argh(option, short = 'a')]
    /// address of server to connect to.
    addr: String,
}

/// main entry point for the client.
pub async fn run_with_opts(opts: CliOpts) {
    let addr = opts.addr;
    let addr = if addr.starts_with("ws://") || addr.starts_with("wss://") {
        addr
    } else {
        format!("ws://{}", addr)
    };

    run_client(addr, opts.username).await;
}

pub enum ClientEvent {
    MouseInput(MouseEvent),
    KeyInput(KeyEvent),
    ServerMessage(message::ToClientMsg),
}

async fn run_client(addr: &str, username: Username) -> error::Result<()> {
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
