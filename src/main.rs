pub mod client;
pub mod data;
pub mod message;
pub mod server;

use argh::FromArgs;
use std::io::{stdout, Write};

use client::app::ServerSession;
use data::Username;
pub use serde::{Deserialize, Serialize};

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
}

#[actix_rt::main]
async fn main() {
    let cli: Opt = argh::from_env();

    match cli.cmd {
        SubOpt::Client(opts) => client::run_with_opts(opts),
        SubOpt::Server(opts) => server::run_with_opts(opts).await.unwrap(),
    }
}
