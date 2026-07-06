use crate::lock::Lock;
use crate::paths::{GlobalDirs, ProjectLayout};
use crate::proclock::ProjectLock;
use crate::runtime::{java, process};
use crate::yml::Snap;
use crate::{LaunchCtx, LoaderSpec};
use anyhow::{Context, Result};
use tracing::info;

pub async fn run(detach: bool) -> Result<()> {
    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;
    std::fs::create_dir_all(layout.snap_dir())?;
    let _guard = ProjectLock::acquire(&layout.lock_file())?;
    let snap = Snap::from_path(&layout.yml())?;
    let lock = Lock::from_path(&layout.lock())
        .context("no mc-snap.lock found; run `mc-snap install` first")?;

    if let Some(pid) = process::read_pid(&layout.pid_file()) {
        if process::is_running_recorded(&layout.pid_file(), pid) {
            anyhow::bail!("server already running (pid {pid})");
        }
        process::clear_pid(&layout.pid_file());
    }

    let required_major = snap.runtime.java.unwrap_or(26);
    let java_install = ensure_java(required_major).await?;
    info!(
        "using java {} at {}",
        java_install.major,
        java_install.bin.display()
    );

    let loader_impl = crate::loaders::for_kind(&snap.server.loader.kind)?;
    let server_dir = layout.server_dir_for(&snap);
    let launch_jar = server_dir.join(match snap.server.loader.kind.as_str() {
        "fabric" => "fabric-server-launch.jar",
        _ => "server.jar",
    });
    let ctx = LaunchCtx {
        server_dir,
        launch_jar,
        java_bin: java_install.bin,
        memory: snap.runtime.memory.clone().unwrap_or_else(|| "2G".into()),
        extra_flags: snap.runtime.flags.clone(),
    };

    let _ = LoaderSpec(snap.server.loader.clone());
    let _ = &lock;
    let cmd = loader_impl.launch_command(&ctx);

    if detach {
        let pid = process::spawn_detached(cmd, &layout.pid_file())?;
        println!(
            "{} started {} (pid {pid})",
            crate::style::ok(),
            crate::style::bold(&snap.server.name)
        );
    } else {
        let code = process::run_foreground(cmd).await?;
        println!("server exited with code {code}");
    }
    Ok(())
}

async fn ensure_java(major: u32) -> Result<java::JavaInstall> {
    if let Some(i) = java::find_matching(major) {
        return Ok(i);
    }
    let globals = GlobalDirs::resolve()?;
    globals.ensure()?;
    if let Some(i) = java::cached_temurin(&globals.jdks, major) {
        return Ok(i);
    }
    info!("no system java {major}; downloading Temurin");
    java::download_temurin(&globals.jdks, major).await
}
