mod game_server;
mod session;
mod skribbl;

use std::{io::Read, result::Result};

use actix::prelude::*;
use argh::FromArgs;
use log::*;
use tokio::{
    net::{TcpListener, TcpStream},
    stream::StreamExt,
};

use self::game_server::GameServer;

const DIMEN: (usize, usize) = (900, 60);
const ROUND_DURATION: usize = 120;
const ROUNDS: usize = 3;
const ROOM_KEY_LENGTH: usize = 5;

#[derive(FromArgs)]
/// host a Termibbl session
#[argh(subcommand, name = "server")]
pub struct CliOpts {
    /// port for server to run on
    #[argh(option, short = 'p')]
    port: u32,

    /// whether to show public ip when server starts
    #[argh(switch, short = 'y')]
    pub display_public_ip: bool,

    #[argh(option, default = "ROUND_DURATION")]
    /// default round duration in seconds
    round_duration: usize,

    #[argh(option, default = "ROUNDS")]
    /// default number of rounds per game
    rounds: usize,

    /// default canvas dimensions <width>x<height>
    #[argh(option, from_str_fn(parse_dimension), default = "DIMEN")]
    dimensions: (usize, usize),

    /// optional path to custom word list
    #[argh(option, from_str_fn(read_words_file))]
    words: Option<Vec<String>>,
}

fn parse_dimension(s: &str) -> Result<(usize, usize), String> {
    let mut split = s
        .split('x')
        .map(str::parse)
        .filter_map(std::result::Result::ok);

    split
        .next()
        .and_then(|width| split.next().map(|height| (width, height)))
        .ok_or("could not parse dimensions".to_owned())
}

fn read_words_file(path: &str) -> Result<Vec<String>, String> {
    info!("reading words from file {}", path);

    let mut words = String::new();
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;

    file.read_to_string(&mut words).map_err(|e| e.to_string())?;

    debug!("read {} words from file", words.len());

    Ok(words
        .lines()
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect::<Vec<String>>())
}

#[derive(Message)]
#[rtype(result = "()")]
struct StopSignal;

#[derive(Clone)]
pub struct GameOpts {
    pub dimensions: (usize, usize),
    pub words: Vec<String>,
    pub number_of_rounds: usize,
    pub round_duration: usize,
}

#[derive(Message)]
#[rtype(result = "()")]
struct TcpConnect(pub TcpStream, pub std::net::SocketAddr);

fn display_public_ip(port: u32) {
    actix::spawn(async move {
        if let Ok(res) = reqwest::get("http://ifconfig.me").await {
            if let Ok(ip) = res.text().await {
                println!("Your public IP is {}:{}", ip, port);
                info!("You can find out your private IP by running \"ip addr\" in the terminal");
            }
        }
    });
}

/// Main entry point for the server
/// Define `GameServer` that will accept incoming tcp connection, create
/// user actors and handle client message.
#[actix_rt::main]
pub async fn run_with_opts(opt: CliOpts) {
    let port = opt.port;
    let default_words = opt.words.unwrap_or_else(Vec::new);
    let default_dimensions = opt.dimensions;
    let default_round_duration = opt.round_duration;
    let default_number_of_rounds = opt.rounds;

    // display public ip
    if opt.display_public_ip {
        display_public_ip(port);
    }

    let default_game_opts = GameOpts {
        dimensions: default_dimensions,
        words: default_words,
        number_of_rounds: default_number_of_rounds,
        round_duration: default_round_duration,
    };

    // start tcp listener on given port
    let addr = format!("127.0.0.1:{}", port);
    let server_listener = Box::new(
        TcpListener::bind(&addr)
            .await
            .expect("Could not start webserver (could not bind)"),
    );

    info!("🚀 Running Termibbl server on {}...", addr);

    // start termibbl server actor
    let game_server = GameServer::create(move |ctx| {
        // listen and handle incoming connections in async thread.
        ctx.add_message_stream(Box::leak(server_listener).incoming().map(|stream| {
            let st = stream.unwrap();
            let addr = st.peer_addr().unwrap();

            TcpConnect(st, addr)
        }));

        GameServer::new(default_game_opts)
    });

    tokio::signal::ctrl_c().await.unwrap();
    println!("Ctrl-C received. Stopping..");

    // gracefully exit
    game_server.do_send(StopSignal);
}
