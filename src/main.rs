use anyhow::Error;
use structopt::StructOpt;

mod actions;
mod benchmark;

#[derive(Debug, StructOpt)]
#[structopt()]
enum Options {
    /// GitHub CI event
    CIEvent {
        #[structopt(short, long, env = "GITHUB_TOKEN")]
        token: String,

        #[structopt(short, long, env = "GITHUB_CONTEXT")]
        context: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    match Options::from_args() {
        Options::CIEvent { token, context } => benchmark::process_event(token, context).await,
    }
}
