mod commands;
mod orchestrate;
mod props;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mc-snap", version, about = "Declarative Minecraft server management")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Init,
    Install,
    Validate,
    Doctor,
    Start {
        #[arg(long)]
        detach: bool,
    },
    Stop,
    Restart,
    Status,
    Logs {
        #[arg(short, long)]
        follow: bool,
    },
    Console {
        command: Vec<String>,
    },
    Pack {
        #[arg(short, long, default_value = "mc-snap-bundle.zip")]
        out: String,
    },
    Unpack {
        bundle: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Init => commands::init::run().await,
        Cmd::Install => commands::install::run().await,
        Cmd::Validate => commands::validate::run().await,
        Cmd::Doctor => commands::doctor::run().await,
        Cmd::Start { detach } => commands::start::run(detach).await,
        Cmd::Stop => commands::stop::run().await,
        Cmd::Restart => commands::restart::run().await,
        Cmd::Status => commands::status::run().await,
        Cmd::Logs { follow } => commands::logs::run(follow).await,
        Cmd::Console { command } => commands::console::run(command).await,
        Cmd::Pack { out } => commands::pack::run(&out).await,
        Cmd::Unpack { bundle } => commands::unpack::run(&bundle).await,
    }
}
