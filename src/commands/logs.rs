use crate::paths::ProjectLayout;
use crate::yml::Snap;
use anyhow::Result;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::time::Duration;

pub async fn run(follow: bool) -> Result<()> {
    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;
    let snap = Snap::from_path(&layout.yml())?;
    let log = layout.server_dir_for(&snap).join("logs").join("latest.log");
    if !log.is_file() {
        anyhow::bail!("no log file at {} yet", log.display());
    }

    if !follow {
        let s = std::fs::read_to_string(&log)?;
        print!("{s}");
        return Ok(());
    }

    let f = std::fs::File::open(&log)?;
    let mut reader = BufReader::new(f);
    reader.seek(SeekFrom::End(0))?;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            tokio::time::sleep(Duration::from_millis(300)).await;
        } else {
            print!("{line}");
        }
    }
}
