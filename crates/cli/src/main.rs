#![forbid(unsafe_code)]

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "dynet",
    version = plumb::version!("DYNET"),
    about = "Dynet command-line boundary"
)]
struct Cli;

fn main() {
    debug_assert_eq!(dynet_api::VERSION, env!("CARGO_PKG_VERSION"));
    Cli::parse();
}
