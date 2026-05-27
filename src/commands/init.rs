use anyhow::Result;
use inquire::ui::{Color, RenderConfig, StyleSheet, Styled};
use inquire::{Confirm, CustomType, Select, Text};
use std::io::IsTerminal;
use std::path::Path;

const TEMPLATE: &str = r#"schema: 1
eula: false

server:
  name: my-server
  description: a mc-snap server
  minecraft: 26.1.2
  loader:
    type: fabric

runtime:
  java: 26
  memory: 4G
  flags:
    - -XX:+UseG1GC

mods:
  - id: fabric-api
    provider: modrinth
    version: latest

config:
  server.properties:
    motd: my-server
    max-players: 20
"#;

pub async fn run(non_interactive: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let target = cwd.join("mc-snap.yml");
    if target.exists() {
        anyhow::bail!("mc-snap.yml already exists at {}", target.display());
    }

    let interactive = !non_interactive && std::io::stdin().is_terminal();
    let body = if interactive {
        wizard(&cwd)?
    } else {
        TEMPLATE.to_string()
    };

    std::fs::write(&target, &body)?;
    ensure_gitignore(&cwd)?;

    println!();
    println!("\x1b[1;32m✓\x1b[0m created \x1b[1m{}\x1b[0m", target.display());
    println!(
        "  next: review the file, set \x1b[1meula: true\x1b[0m after reading \x1b[36mhttps://www.minecraft.net/en-us/eula\x1b[0m,"
    );
    println!("        then run \x1b[1mmc-snap install\x1b[0m");
    Ok(())
}

fn wizard(cwd: &Path) -> Result<String> {
    inquire::set_global_render_config(render_config());

    let dir_default = cwd
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("my-server")
        .to_string();

    println!();
    println!("\x1b[1;36m  mc-snap\x1b[0m \x1b[2m— new server config\x1b[0m");
    println!("\x1b[2m  arrow keys to navigate, enter to confirm, ctrl-c to cancel\x1b[0m");
    println!();

    let name = Text::new("Server name")
        .with_default(&dir_default)
        .with_validator(inquire::required!("server name is required"))
        .prompt()?;

    let description = Text::new("Description")
        .with_default("a mc-snap server")
        .prompt()?;

    let minecraft = Text::new("Minecraft version")
        .with_default("26.1.2")
        .with_help_message("e.g. 26.1.2")
        .with_validator(inquire::required!("minecraft version is required"))
        .prompt()?;

    let loader = Select::new("Loader", vec!["fabric", "vanilla"])
        .with_help_message("fabric supports mods; vanilla is mod-free")
        .prompt()?;

    let java = CustomType::<u32>::new("Java version")
        .with_default(26)
        .with_help_message("major version only, e.g. 21 or 26")
        .with_error_message("enter a positive integer")
        .prompt()?;

    let memory = Text::new("Memory")
        .with_default("4G")
        .with_help_message("e.g. 2G, 4G, 8G")
        .with_validator(inquire::required!("memory is required"))
        .prompt()?;

    let motd = Text::new("MOTD")
        .with_default(&name)
        .prompt()?;

    let max_players = CustomType::<u32>::new("Max players")
        .with_default(20)
        .with_error_message("enter a positive integer")
        .prompt()?;

    let include_fabric_api = if loader == "fabric" {
        Confirm::new("Include fabric-api?")
            .with_default(true)
            .with_help_message("the core mod API most fabric mods depend on")
            .prompt()?
    } else {
        false
    };

    let eula = Confirm::new("Accept the Minecraft EULA?")
        .with_default(false)
        .with_help_message("https://www.minecraft.net/en-us/eula — required before starting the server")
        .prompt()?;

    Ok(render_yml(
        &name,
        &description,
        &minecraft,
        loader,
        java,
        &memory,
        &motd,
        max_players,
        include_fabric_api,
        eula,
    ))
}

#[allow(clippy::too_many_arguments)]
fn render_yml(
    name: &str,
    description: &str,
    minecraft: &str,
    loader: &str,
    java: u32,
    memory: &str,
    motd: &str,
    max_players: u32,
    include_fabric_api: bool,
    eula: bool,
) -> String {
    let mut out = String::new();
    out.push_str("schema: 1\n");
    out.push_str(&format!("eula: {eula}\n\n"));

    out.push_str("server:\n");
    out.push_str(&format!("  name: {}\n", yaml_scalar(name)));
    if !description.trim().is_empty() {
        out.push_str(&format!("  description: {}\n", yaml_scalar(description)));
    }
    out.push_str(&format!("  minecraft: {}\n", yaml_scalar(minecraft)));
    out.push_str("  loader:\n");
    out.push_str(&format!("    type: {loader}\n"));
    out.push('\n');

    out.push_str("runtime:\n");
    out.push_str(&format!("  java: {java}\n"));
    out.push_str(&format!("  memory: {}\n", yaml_scalar(memory)));
    out.push_str("  flags:\n");
    out.push_str("    - -XX:+UseG1GC\n");
    out.push('\n');

    if include_fabric_api {
        out.push_str("mods:\n");
        out.push_str("  - id: fabric-api\n");
        out.push_str("    provider: modrinth\n");
        out.push_str("    version: latest\n");
        out.push('\n');
    } else {
        out.push_str("mods: []\n\n");
    }

    out.push_str("config:\n");
    out.push_str("  server.properties:\n");
    out.push_str(&format!("    motd: {}\n", yaml_scalar(motd)));
    out.push_str(&format!("    max-players: {max_players}\n"));

    out
}

fn yaml_scalar(s: &str) -> String {
    let needs_quote = s.is_empty()
        || s.contains(':')
        || s.contains('#')
        || s.contains('"')
        || s.contains('\\')
        || s.starts_with(' ')
        || s.ends_with(' ')
        || s.starts_with('-')
        || s.starts_with(|c: char| {
            matches!(
                c,
                '!' | '&' | '*' | '[' | '{' | '|' | '>' | '\'' | '%' | '@' | '`'
            )
        });
    if needs_quote {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

fn render_config() -> RenderConfig<'static> {
    RenderConfig::default()
        .with_prompt_prefix(Styled::new("›").with_fg(Color::LightCyan))
        .with_answered_prompt_prefix(Styled::new("✓").with_fg(Color::LightGreen))
        .with_help_message(StyleSheet::new().with_fg(Color::DarkGrey))
        .with_answer(StyleSheet::new().with_fg(Color::LightCyan))
        .with_default_value(StyleSheet::new().with_fg(Color::DarkGrey))
}

fn ensure_gitignore(dir: &Path) -> Result<()> {
    let path = dir.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == ".mc-snap") || existing.lines().any(|l| l.trim() == ".mc-snap/") {
        return Ok(());
    }
    let mut new = existing;
    if !new.ends_with('\n') && !new.is_empty() {
        new.push('\n');
    }
    new.push_str(".mc-snap/\n");
    std::fs::write(path, new)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yml::Snap;

    #[test]
    fn rendered_yml_parses_and_round_trips() {
        let body = render_yml(
            "grimwald",
            "the grimwald smp",
            "26.1.2",
            "fabric",
            26,
            "4G",
            "welcome!",
            20,
            true,
            true,
        );
        let snap = Snap::from_str(&body).unwrap();
        assert_eq!(snap.server.name, "grimwald");
        assert_eq!(snap.server.minecraft, "26.1.2");
        assert_eq!(snap.server.loader.kind, "fabric");
        assert_eq!(snap.runtime.java, Some(26));
        assert_eq!(snap.mods.len(), 1);
        assert!(snap.eula);
    }

    #[test]
    fn vanilla_without_fabric_api_has_empty_mods() {
        let body = render_yml(
            "vanilla-svr",
            "",
            "26.1.2",
            "vanilla",
            26,
            "2G",
            "hi",
            10,
            false,
            false,
        );
        let snap = Snap::from_str(&body).unwrap();
        assert_eq!(snap.server.loader.kind, "vanilla");
        assert!(snap.mods.is_empty());
        assert!(!snap.eula);
        assert!(snap.server.description.is_none() || snap.server.description.as_deref() == Some(""));
    }

    #[test]
    fn quotes_values_with_special_chars() {
        let body = render_yml(
            "weird:name",
            "has # hash",
            "26.1.2",
            "fabric",
            26,
            "4G",
            "say \"hi\"",
            20,
            false,
            false,
        );
        let snap = Snap::from_str(&body).unwrap();
        assert_eq!(snap.server.name, "weird:name");
    }

    #[test]
    fn yaml_scalar_passes_simple_strings_through() {
        assert_eq!(yaml_scalar("hello"), "hello");
        assert_eq!(yaml_scalar("my-server"), "my-server");
        assert_eq!(yaml_scalar("-leading-dash"), "\"-leading-dash\"");
        assert_eq!(yaml_scalar(""), "\"\"");
        assert_eq!(yaml_scalar("a:b"), "\"a:b\"");
        assert_eq!(yaml_scalar("has # hash"), "\"has # hash\"");
    }
}
