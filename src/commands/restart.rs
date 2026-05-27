use anyhow::Result;
use crate::paths::ProjectLayout;
use crate::yml::Snap;
use std::time::Duration;

pub async fn run() -> Result<()> {
    super::stop::run().await.ok();

    // After stop, the OS may not have released the listen socket yet — especially
    // on Windows where TerminateProcess returns before the socket actually closes.
    // Probe the configured server port and wait briefly for it to become free.
    if let Ok(layout) = ProjectLayout::discover(&std::env::current_dir()?) {
        if let Ok(snap) = Snap::from_path(&layout.yml()) {
            let port = snap
                .config
                .server_properties
                .get("server-port")
                .and_then(|v| v.as_i64())
                .unwrap_or(25565) as u16;
            wait_for_port_free(port, Duration::from_secs(10)).await;
        }
    }

    super::start::run(true).await
}

async fn wait_for_port_free(port: u16, timeout: Duration) {
    use tokio::net::TcpListener;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        // If we can bind, the previous server has released the port.
        if TcpListener::bind(("127.0.0.1", port)).await.is_ok() {
            return;
        }
        if std::time::Instant::now() >= deadline {
            tracing::warn!("port {port} still in use after restart wait; starting anyway");
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}
