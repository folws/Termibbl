pub use crate::*;
pub mod app;
pub mod error;
pub mod ui;

use argh::FromArgs;

#[derive(FromArgs)]
/// play Skribbl.io-like games in the Termibbl
#[argh(subcommand)]
pub struct CliOpts {
    #[argh(positional)]
    ///username to connect as.
    username: String,

    #[argh(option, short = 'a')]
    /// address of server to connect to.
    addr: String,
}

/// main entry point for the client.
pub fn run_with_opts(opts: CliOpts) {
    let addr = opts.addr;
    let addr = if addr.starts_with("ws://") || addr.starts_with("wss://") {
        addr
    } else {
        format!("ws://{}", addr)
    };
}
