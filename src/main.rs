use anyhow::Result;
use clap::{Parser, Subcommand};
use mc_snap::commands;

#[derive(Parser)]
#[command(
    name = "mc-snap",
    version,
    about = "Declarative Minecraft server management"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a new mc-snap.yml in the current directory (interactive wizard).
    Init {
        /// Skip the interactive wizard and write the default template.
        #[arg(long)]
        non_interactive: bool,
        /// Detect server structure in PATH (defaults to current dir) and pre-fill the wizard.
        #[arg(long, value_name = "PATH", num_args = 0..=1, default_missing_value = ".")]
        detect: Option<String>,
        /// Overwrite an existing mc-snap.yml without prompting.
        #[arg(long)]
        force: bool,
        /// Skip Modrinth lookups when detecting mods (offline mode).
        #[arg(long)]
        no_mod_resolve: bool,
    },
    /// Resolve versions, download artifacts, and materialize the server directory.
    /// Reuses the existing lockfile when mc-snap.yml is unchanged.
    Install {
        /// Copy artifacts into the server dir instead of linking them from the cache.
        #[arg(long, conflicts_with = "symlink")]
        copy: bool,
        /// Symlink artifacts on every platform (default: symlink on Unix, hardlink on Windows).
        #[arg(long)]
        symlink: bool,
        /// Re-resolve all versions even if mc-snap.yml is unchanged (refreshes `latest` pins).
        #[arg(long)]
        refresh: bool,
    },
    /// Parse and validate mc-snap.yml without touching the network.
    Validate,
    /// Report discovered Java installs, cache paths, and project status.
    Doctor,
    /// Start the server (foreground by default).
    Start {
        /// Run the server in the background and record its pid.
        #[arg(long)]
        detach: bool,
    },
    /// Stop the server gracefully via RCON, escalating to signals if needed.
    Stop,
    /// Stop the server, wait for the port to free up, then start detached.
    Restart,
    /// Show whether the server is running and who is online.
    Status,
    /// Print the server log (logs/latest.log).
    Logs {
        /// Keep the log open and print new lines as they appear.
        #[arg(short, long)]
        follow: bool,
    },
    /// Send an RCON command, or open an interactive console with no arguments.
    Console { command: Vec<String> },
    /// Bundle mc-snap.yml + mc-snap.lock + configs/ into a shareable zip.
    Pack {
        #[arg(short, long, default_value = "mc-snap-bundle.zip")]
        out: String,
    },
    /// Extract a bundle into the current directory.
    Unpack {
        bundle: String,
        /// Overwrite existing files.
        #[arg(long)]
        force: bool,
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
    /// Add a mod by slug, picking the newest version compatible with the
    /// configured Minecraft + loader, then run install. Pass multiple slugs to
    /// add several at once. `--version` pins a specific release (single slug only).
    Get {
        /// Mod slug(s) on the provider (e.g. `fabric-api`, `chunky`).
        #[arg(required = true)]
        slugs: Vec<String>,
        /// Pin a specific provider version instead of picking the newest.
        #[arg(long)]
        version: Option<String>,
        /// Provider to query. Defaults to `modrinth`.
        #[arg(long)]
        provider: Option<String>,
        /// Edit mc-snap.yml only; skip the install step.
        #[arg(long)]
        no_install: bool,
    },
    /// Manage tracked config files (e.g. mod configs under <server>/config).
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Scan the server's config/ directory and offer to track new files.
    Detect {
        /// Skip prompts and track every untracked config file.
        #[arg(long)]
        all: bool,
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
        Cmd::Init {
            non_interactive,
            detect,
            force,
            no_mod_resolve,
        } => commands::init::run(non_interactive, detect, force, no_mod_resolve).await,
        Cmd::Install {
            copy,
            symlink,
            refresh,
        } => {
            let mode = if copy {
                mc_snap::cache::LinkMode::Copy
            } else if symlink {
                mc_snap::cache::LinkMode::Symlink
            } else {
                mc_snap::cache::LinkMode::Auto
            };
            commands::install::run(mode, refresh).await
        }
        Cmd::Validate => commands::validate::run().await,
        Cmd::Doctor => commands::doctor::run().await,
        Cmd::Start { detach } => commands::start::run(detach).await,
        Cmd::Stop => commands::stop::run().await,
        Cmd::Restart => commands::restart::run().await,
        Cmd::Status => commands::status::run().await,
        Cmd::Logs { follow } => commands::logs::run(follow).await,
        Cmd::Console { command } => commands::console::run(command).await,
        Cmd::Pack { out } => commands::pack::run(&out).await,
        Cmd::Unpack { bundle, force } => commands::unpack::run(&bundle, force).await,
        Cmd::Update {
            to,
            skip_missing,
            yes,
            loader,
        } => commands::update::run(&to, skip_missing, yes, loader).await,
        Cmd::Revert { id, list } => commands::revert::run(id, list).await,
        Cmd::Check { to } => commands::check::run(&to).await,
        Cmd::Updatable { to } => commands::updatable::run(to).await,
        Cmd::Search => commands::search::run().await,
        Cmd::Get {
            slugs,
            version,
            provider,
            no_install,
        } => commands::get::run(slugs, version, provider, no_install).await,
        Cmd::Config { cmd } => match cmd {
            ConfigCmd::Detect { all } => commands::config::run_detect(all).await,
        },
    }
}
