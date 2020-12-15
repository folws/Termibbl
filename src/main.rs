pub mod client;
pub mod data;
pub mod error;
pub mod message;
pub mod server;

use std::io::{stdout, Write};
use std::path::PathBuf;
use structopt::StructOpt;

use client::app::App;
use client::app::ServerSession;
use data::Username;
pub use serde::{Deserialize, Serialize};
use server::constants::ROUND_DURATION;

#[derive(Debug, StructOpt)]
#[structopt(name = "Termibbl", about = "A Skribbl.io-alike for the terminal")]
struct Opt {
    #[structopt(subcommand)]
    cmd: CliOpts,
}

#[derive(Debug, StructOpt)]
enum CliOpts {
    Server {
        #[structopt(long = "--port", short = "-p")]
        port: u32,
        #[structopt(long)]
        display_public_ip: bool,
        #[structopt(long = "--words", parse(from_os_str), required_if("freedraw", "true"))]
        word_file: Option<PathBuf>,
        #[structopt(short, long, help = "<width>x<height>", parse(from_str = crate::parse_dimension), default_value = "100x50")]
        dimensions: (usize, usize),
        // #[structopt(default_value = "120", long = "--duration", short = "-d")]
        // round_duration: u32,
        // #[structopt(long, short = "-r")]
        // rounds: u32,
    },
    Client {
        #[structopt(long = "address", short = "-a")]
        addr: String,
        username: String,
    },
}

fn parse_dimension(s: &str) -> (usize, usize) {
    let mut split = s.split('x');
    (
        split.next().unwrap().parse().unwrap(),
        split.next().unwrap().parse().unwrap(),
    )
}

#[actix_rt::main]
async fn main() {
    pretty_env_logger::init();

    let opt = Opt::from_args();
    match opt.cmd {
        CliOpts::Client { username, addr } => {
            let addr = if addr.starts_with("ws://") || addr.starts_with("wss://") {
                addr
            } else {
                format!("ws://{}", addr)
            };

            client::run(&addr, username.into())?;
        }
        CliOpts::Server {
            port,
            display_public_ip,
            word_file,
            dimensions,
            // round_duration,
            // rounds,
        } => server::run(
            port,
            display_public_ip,
            word_file,
            dimensions,
            ROUND_DURATION,
            5,
        )?,
    }
}
