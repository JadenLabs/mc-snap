use crate::orchestrate;
use anyhow::Result;
use mc_snap_core::paths::ProjectLayout;
use mc_snap_core::yml::Snap;
use mc_snap_runtime::rcon;

pub async fn run(command: Vec<String>) -> Result<()> {
    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;
    let snap = Snap::from_path(&layout.yml())?;
    let pw = orchestrate::read_rcon_password(&layout)?;
    let addr = orchestrate::rcon_address(&snap);
    let mut r = rcon::Rcon::connect(&addr, &pw).await?;

    if command.is_empty() {
        use std::io::{BufRead, Write};
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout();
        println!("connected to {addr}. type commands, ctrl-d to exit.");
        loop {
            print!("> ");
            stdout.flush().ok();
            let mut line = String::new();
            if stdin.lock().read_line(&mut line)? == 0 {
                break;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let out = r.exec(trimmed).await?;
            if !out.is_empty() {
                println!("{out}");
            }
        }
    } else {
        let out = r.exec(&command.join(" ")).await?;
        if !out.is_empty() {
            println!("{out}");
        }
    }
    Ok(())
}
