use anyhow::Error;
use structopt::StructOpt;

mod github_actions;

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
        Options::CIEvent { token, context } => github_actions::process_event(token, context).await,
    }
}
