mod commands;
mod compat;
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
    /// Update to a newer Minecraft version. Snapshots current state first so it can be reverted.
    Update {
        /// Target Minecraft version (e.g. 26.1.3).
        #[arg(long)]
        to: String,
        /// Drop mods that have no version for the target without prompting.
        #[arg(long)]
        skip_missing: bool,
        /// Don't prompt; cancel instead if any mods are missing.
        #[arg(short = 'y', long)]
        yes: bool,
        /// Pin a specific loader version (otherwise resolves to latest stable).
        #[arg(long)]
        loader: Option<String>,
    },
    /// Revert to a previous snapshot taken by `update`.
    Revert {
        /// Snapshot id; defaults to the most recent.
        id: Option<String>,
        /// List snapshots instead of reverting.
        #[arg(long)]
        list: bool,
    },
    /// Check mod compatibility against a given Minecraft version (no filesystem changes).
    Check {
        /// Target Minecraft version.
        #[arg(long)]
        to: String,
    },
    /// Report whether this modpack can be updated. With `--to`, answers yes/no for that version;
    /// without, suggests the newest Minecraft version supported by every mod.
    Updatable {
        #[arg(long)]
        to: Option<String>,
    },
    /// List newer mod versions available for the current Minecraft version.
    Search,
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
        Cmd::Update { to, skip_missing, yes, loader } => {
            commands::update::run(&to, skip_missing, yes, loader).await
        }
        Cmd::Revert { id, list } => commands::revert::run(id, list).await,
        Cmd::Check { to } => commands::check::run(&to).await,
        Cmd::Updatable { to } => commands::updatable::run(to).await,
        Cmd::Search => commands::search::run().await,
    }
}
