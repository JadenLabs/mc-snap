use crate::orchestrate;
use crate::paths::ProjectLayout;
use crate::runtime::rcon;
use crate::yml::Snap;
use anyhow::Result;

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
        let eof_hint = if cfg!(windows) {
            "ctrl-z then enter"
        } else {
            "ctrl-d"
        };
        println!("connected to {addr}. type commands, {eof_hint} to exit.");
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
            // A failed command shouldn't kill the whole shell; report and keep going.
            match r.exec(trimmed).await {
                Ok(out) => {
                    if !out.is_empty() {
                        println!("{out}");
                    }
                }
                Err(e) => eprintln!("error: {e:#}"),
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
