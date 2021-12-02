use anyhow::Error;
use std::path::PathBuf;
use structopt::StructOpt;

mod actions;
mod benchmark;
mod resctl_bench;

#[derive(StructOpt)]
struct Options {
    #[structopt(short, long, env = "RESCTL_BENCH", default_value = "resctl-bench")]
    resctl_bench: String,

    #[structopt(subcommand)]
    subcommand: Command,
}

#[derive(StructOpt)]
enum Command {
    /// GitHub CI event
    CIEvent {
        #[structopt(short, long, env = "GITHUB_TOKEN")]
        token: String,

        #[structopt(short, long, env = "GITHUB_CONTEXT")]
        context: String,
    },

    TestMerge {
        /// Input files
        #[structopt(parse(from_os_str))]
        input_files: Vec<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let options = Options::from_args();

    match options.subcommand {
        Command::CIEvent { token, context } => {
            benchmark::process_event(options.resctl_bench, token, context).await
        }
        Command::TestMerge { input_files } => {
            resctl_bench::merge(options.resctl_bench, input_files).await
        }
    }
}
