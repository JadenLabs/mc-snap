mod detect;

use anyhow::Result;
use inquire::ui::{Color, RenderConfig, StyleSheet, Styled};
use inquire::{Confirm, CustomType, Select, Text};
use serde_yml::{Mapping, Value};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use crate::style;
use crate::yml::{ConfigSection, DatapackEntry, FileRef, Loader, ModEntry, Runtime, Server, Snap};
use detect::Detected;

pub use detect::detect as detect_path;

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

pub async fn run(
    non_interactive: bool,
    detect_path: Option<String>,
    force: bool,
    no_mod_resolve: bool,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let target = cwd.join("mc-snap.yml");
    let interactive = !non_interactive && std::io::stdin().is_terminal();

    if target.exists() {
        if force {
            // overwrite silently
        } else if interactive {
            inquire::set_global_render_config(render_config());
            let ok = Confirm::new(&format!(
                "mc-snap.yml exists at {} - overwrite?",
                target.display()
            ))
            .with_default(false)
            .prompt()?;
            if !ok {
                anyhow::bail!("aborted: mc-snap.yml already exists");
            }
        } else {
            anyhow::bail!(
                "mc-snap.yml already exists at {} (use --force to overwrite)",
                target.display()
            );
        }
    }

    let detected = if let Some(p) = detect_path {
        let root = PathBuf::from(&p);
        if !root.is_dir() {
            anyhow::bail!("--detect path is not a directory: {}", root.display());
        }
        println!();
        println!(
            "{} {}",
            style::heading("  mc-snap"),
            style::dim(&format!("- detecting in {}", root.display()))
        );
        let d = detect::detect(&root, !no_mod_resolve).await?;
        print_detection_summary(&d, no_mod_resolve);
        Some(d)
    } else {
        None
    };

    let body = match (&detected, interactive) {
        (Some(d), true) => wizard(&cwd, Some(d))?,
        (Some(d), false) => render_from_detected(&cwd, d),
        (None, true) => wizard(&cwd, None)?,
        (None, false) => TEMPLATE.to_string(),
    };

    std::fs::write(&target, &body)?;
    ensure_gitignore(&cwd)?;

    let written_location = crate::yml::Snap::from_str(&body)
        .ok()
        .and_then(|s| s.server.location);
    print_gitignore_hint(written_location.as_deref());

    println!();
    println!(
        "{} created {}",
        style::ok(),
        style::bold(&target.display().to_string())
    );
    println!(
        "  next: review the file, set {} after reading {},",
        style::bold("eula: true"),
        style::cyan("https://www.minecraft.net/en-us/eula")
    );
    println!("        then run {}", style::bold("mc-snap install"));
    Ok(())
}

fn print_gitignore_hint(location: Option<&str>) {
    let prefix = match location {
        Some(sub) if !sub.trim().is_empty() && sub.trim() != "." => {
            format!("{}/", sub.trim().trim_end_matches('/'))
        }
        _ => String::new(),
    };
    println!();
    println!(
        "{}",
        style::dim(&format!(
            "  hint: server artifacts will land at {}",
            if prefix.is_empty() {
                "the project root".to_string()
            } else {
                format!("./{prefix}")
            }
        ))
    );
    println!("{}", style::dim("        consider adding to .gitignore:"));
    for entry in [
        "world/",
        "world_nether/",
        "world_the_end/",
        "logs/",
        "crash-reports/",
        "mods/",
        "*.jar",
        "server.properties",
        "eula.txt",
        "usercache.json",
        "ops.json",
        "banned-players.json",
        "banned-ips.json",
        "whitelist.json",
    ] {
        println!("{}", style::dim(&format!("          {prefix}{entry}")));
    }
}

fn print_detection_summary(d: &Detected, no_mod_resolve: bool) {
    let bullet = style::ok();
    let warn = style::warn_glyph();
    if let Some(unsup) = d.unsupported_loader {
        println!(
            "  {warn} detected {} server - mc-snap currently supports only vanilla and fabric",
            unsup
        );
    }
    if let Some(loader) = d.loader {
        match (loader, d.fabric_loader_version.as_deref()) {
            ("fabric", Some(v)) => println!("  {bullet} loader: fabric {v}"),
            (l, _) => println!("  {bullet} loader: {l}"),
        }
    }
    if let Some(mc) = &d.minecraft {
        println!("  {bullet} minecraft: {mc}");
    }
    if let Some(j) = d.java_major {
        println!("  {bullet} java: {j}");
    }
    if let Some(mem) = &d.memory {
        println!("  {bullet} memory: {mem}");
    }
    if d.eula {
        println!("  {bullet} eula: accepted");
    }
    if !d.server_properties.is_empty() {
        println!(
            "  {bullet} server.properties: {} entries",
            d.server_properties.len()
        );
    }
    if !d.mods.is_empty() || !d.unresolved_mods.is_empty() {
        if no_mod_resolve {
            println!(
                "  {bullet} mods: {} jar(s) listed (resolve skipped)",
                d.unresolved_mods.len()
            );
        } else {
            println!(
                "  {bullet} mods: {} resolved via Modrinth, {} unresolved",
                d.mods.len(),
                d.unresolved_mods.len()
            );
            for name in &d.unresolved_mods {
                println!("    {} {name}", style::dim("-"));
            }
        }
    }
    if !d.datapacks.is_empty() || !d.unresolved_datapacks.is_empty() {
        if no_mod_resolve {
            println!(
                "  {bullet} datapacks: {} zip(s) listed (resolve skipped)",
                d.unresolved_datapacks.len()
            );
        } else {
            println!(
                "  {bullet} datapacks: {} resolved via Modrinth, {} unresolved",
                d.datapacks.len(),
                d.unresolved_datapacks.len()
            );
            for name in &d.unresolved_datapacks {
                println!("    {} {name}", style::dim("-"));
            }
        }
    }
    println!();
}

fn wizard(cwd: &Path, detected: Option<&Detected>) -> Result<String> {
    inquire::set_global_render_config(render_config());

    let dir_default = detected
        .and_then(|d| d.name.clone())
        .or_else(|| {
            cwd.file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "my-server".to_string());

    println!();
    println!(
        "{} {}",
        style::heading("  mc-snap"),
        style::dim("- new server config")
    );
    println!(
        "{}",
        style::dim("  arrow keys to navigate, enter to confirm, ctrl-c to cancel")
    );
    println!();

    let name = Text::new("Server name")
        .with_default(&dir_default)
        .with_validator(inquire::required!("server name is required"))
        .prompt()?;

    let description = Text::new("Description")
        .with_default("a mc-snap server")
        .prompt()?;

    let mc_default = detected
        .and_then(|d| d.minecraft.clone())
        .unwrap_or_else(|| "26.1.2".to_string());
    let minecraft = Text::new("Minecraft version")
        .with_default(&mc_default)
        .with_help_message("e.g. 26.1.2")
        .with_validator(inquire::required!("minecraft version is required"))
        .prompt()?;

    let loader_default_idx = match detected.and_then(|d| d.loader) {
        Some("vanilla") => 1,
        _ => 0,
    };
    let loader = Select::new("Loader", vec!["fabric", "vanilla"])
        .with_starting_cursor(loader_default_idx)
        .with_help_message("fabric supports mods; vanilla is mod-free")
        .prompt()?;

    let loc_default = detected
        .and_then(|d| d.detected_location.clone())
        .unwrap_or_else(|| ".".to_string());
    let location_input = Text::new("Server directory")
        .with_default(&loc_default)
        .with_help_message("'.' for current directory, or a subdir like 'server'")
        .prompt()?;
    let location = normalize_location(&location_input);

    let java_default = detected.and_then(|d| d.java_major).unwrap_or(26);
    let java = CustomType::<u32>::new("Java version")
        .with_default(java_default)
        .with_help_message("major version only, e.g. 21 or 26")
        .with_error_message("enter a positive integer")
        .prompt()?;

    let mem_default = detected
        .and_then(|d| d.memory.clone())
        .unwrap_or_else(|| "4G".to_string());
    let memory = Text::new("Memory")
        .with_default(&mem_default)
        .with_help_message("e.g. 2G, 4G, 8G")
        .with_validator(inquire::required!("memory is required"))
        .prompt()?;

    let motd_detected = detected.and_then(|d| {
        d.server_properties
            .get(Value::String("motd".to_string()))
            .and_then(|v| v.as_str().map(|s| s.to_string()))
    });
    let motd_default = motd_detected.unwrap_or_else(|| name.clone());
    let motd = Text::new("MOTD").with_default(&motd_default).prompt()?;

    let max_players_detected = detected.and_then(|d| {
        d.server_properties
            .get(Value::String("max-players".to_string()))
            .and_then(|v| v.as_i64())
            .and_then(|n| u32::try_from(n).ok())
    });
    let max_players = CustomType::<u32>::new("Max players")
        .with_default(max_players_detected.unwrap_or(20))
        .with_error_message("enter a positive integer")
        .prompt()?;

    // If detection already found a mod list, don't ask about fabric-api again - keep what we found.
    let detected_has_mods = detected.is_some_and(|d| !d.mods.is_empty());
    let include_fabric_api = if loader == "fabric" && !detected_has_mods {
        Confirm::new("Include fabric-api?")
            .with_default(true)
            .with_help_message("the core mod API most fabric mods depend on")
            .prompt()?
    } else {
        false
    };

    let eula_default = detected.is_some_and(|d| d.eula);
    let eula = Confirm::new("Accept the Minecraft EULA?")
        .with_default(eula_default)
        .with_help_message(
            "https://www.minecraft.net/en-us/eula - required before starting the server",
        )
        .prompt()?;

    let mods: Vec<ModEntry> = if detected_has_mods {
        detected.unwrap().mods.clone()
    } else if include_fabric_api {
        vec![ModEntry::Registry {
            id: "fabric-api".to_string(),
            provider: "modrinth".to_string(),
            version: "latest".to_string(),
        }]
    } else {
        Vec::new()
    };

    let mut props = detected
        .map(|d| d.server_properties.clone())
        .unwrap_or_default();
    set_prop(&mut props, "motd", Value::String(motd));
    set_prop(
        &mut props,
        "max-players",
        Value::Number((max_players as i64).into()),
    );

    let mut snap = build_snap(
        &name,
        &description,
        &minecraft,
        loader,
        location.as_deref(),
        java,
        &memory,
        &mods,
        &props,
        eula,
    );

    if let Some(d) = detected {
        snap.config.files = scan_and_select_configs(cwd, &snap, d)?;
    }

    Ok(render_yml(&snap))
}

fn scan_and_select_configs(cwd: &Path, snap: &Snap, detected: &Detected) -> Result<Vec<FileRef>> {
    use crate::commands::config::{scan_config_dir, track_candidate};
    use inquire::MultiSelect;

    let server_dir = detected_server_dir(cwd, snap, detected);
    let candidates = scan_config_dir(&server_dir).unwrap_or_default();
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    println!();
    println!(
        "{} {}",
        style::heading("  config"),
        style::dim(&format!(
            "- found {} file(s) under {}/",
            candidates.len(),
            server_dir.join("config").display()
        ))
    );

    let labels: Vec<String> = candidates.iter().map(|c| c.server_rel.clone()).collect();
    let defaults: Vec<usize> = (0..labels.len()).collect();
    let chosen = MultiSelect::new("Track which configs?", labels.clone())
        .with_default(&defaults)
        .with_help_message("space toggles, enter confirms - default tracks everything")
        .prompt()?;

    let chosen_set: std::collections::HashSet<&str> = chosen.iter().map(|s| s.as_str()).collect();
    let mut out = Vec::with_capacity(chosen.len());
    for c in &candidates {
        if chosen_set.contains(c.server_rel.as_str()) {
            out.push(track_candidate(cwd, c)?);
        }
    }
    Ok(out)
}

fn scan_configs_all(cwd: &Path, snap: &Snap, detected: &Detected) -> Result<Vec<FileRef>> {
    use crate::commands::config::{scan_config_dir, track_candidate};
    let server_dir = detected_server_dir(cwd, snap, detected);
    let candidates = scan_config_dir(&server_dir).unwrap_or_default();
    let mut out = Vec::with_capacity(candidates.len());
    for c in &candidates {
        out.push(track_candidate(cwd, c)?);
    }
    Ok(out)
}

fn detected_server_dir(cwd: &Path, snap: &Snap, detected: &Detected) -> PathBuf {
    if let Some(loc) = detected.detected_location.as_deref() {
        return cwd.join(loc);
    }
    match snap.server.location.as_deref() {
        Some(sub) if !sub.trim().is_empty() && sub.trim() != "." => cwd.join(sub),
        _ => cwd.to_path_buf(),
    }
}

fn normalize_location(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() || t == "." {
        None
    } else {
        Some(t.to_string())
    }
}

fn render_from_detected(cwd: &Path, d: &Detected) -> String {
    let name = d
        .name
        .clone()
        .or_else(|| {
            cwd.file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "my-server".to_string());
    let description = "a mc-snap server".to_string();
    let minecraft = d.minecraft.clone().unwrap_or_else(|| "26.1.2".to_string());
    let loader = d.loader.unwrap_or("fabric");
    let java = d.java_major.unwrap_or(26);
    let memory = d.memory.clone().unwrap_or_else(|| "4G".to_string());

    let mut props = d.server_properties.clone();
    if !props.contains_key(Value::String("motd".to_string())) {
        set_prop(&mut props, "motd", Value::String(name.clone()));
    }
    if !props.contains_key(Value::String("max-players".to_string())) {
        set_prop(&mut props, "max-players", Value::Number(20i64.into()));
    }

    let mut snap = build_snap(
        &name,
        &description,
        &minecraft,
        loader,
        d.detected_location.as_deref(),
        java,
        &memory,
        &d.mods,
        &props,
        d.eula,
    );
    snap.datapacks = d.datapacks.clone();
    snap.config.files = scan_configs_all(cwd, &snap, d).unwrap_or_default();
    render_yml(&snap)
}

fn set_prop(m: &mut Mapping, key: &str, value: Value) {
    m.insert(Value::String(key.to_string()), value);
}

pub fn render_yml(snap: &Snap) -> String {
    let mut out = String::new();
    out.push_str(&format!("schema: {}\n", snap.schema));
    out.push_str(&format!("eula: {}\n\n", snap.eula));

    out.push_str("server:\n");
    out.push_str(&format!("  name: {}\n", yaml_scalar(&snap.server.name)));
    if let Some(desc) = snap.server.description.as_deref() {
        if !desc.trim().is_empty() {
            out.push_str(&format!("  description: {}\n", yaml_scalar(desc)));
        }
    }
    out.push_str(&format!(
        "  minecraft: {}\n",
        yaml_scalar(&snap.server.minecraft)
    ));
    if let Some(loc) = snap.server.location.as_deref() {
        out.push_str(&format!("  location: {}\n", yaml_scalar(loc)));
    }
    out.push_str("  loader:\n");
    out.push_str(&format!(
        "    type: {}\n",
        yaml_scalar(&snap.server.loader.kind)
    ));
    if let Some(v) = snap.server.loader.version.as_deref() {
        out.push_str(&format!("    version: {}\n", yaml_scalar(v)));
    }
    if let Some(v) = snap.server.loader.installer.as_deref() {
        out.push_str(&format!("    installer: {}\n", yaml_scalar(v)));
    }
    out.push('\n');

    out.push_str("runtime:\n");
    if let Some(j) = snap.runtime.java {
        out.push_str(&format!("  java: {j}\n"));
    }
    if let Some(mem) = snap.runtime.memory.as_deref() {
        out.push_str(&format!("  memory: {}\n", yaml_scalar(mem)));
    }
    if snap.runtime.flags.is_empty() {
        out.push_str("  flags:\n    - -XX:+UseG1GC\n");
    } else {
        out.push_str("  flags:\n");
        for f in &snap.runtime.flags {
            out.push_str(&format!("    - {}\n", yaml_scalar(f)));
        }
    }
    out.push('\n');

    if snap.mods.is_empty() {
        out.push_str("mods: []\n\n");
    } else {
        out.push_str("mods:\n");
        for entry in &snap.mods {
            match entry {
                ModEntry::Registry {
                    id,
                    provider,
                    version,
                } => {
                    out.push_str(&format!("  - id: {}\n", yaml_scalar(id)));
                    out.push_str(&format!("    provider: {}\n", yaml_scalar(provider)));
                    out.push_str(&format!("    version: {}\n", yaml_scalar(version)));
                }
                ModEntry::Url {
                    url,
                    provider,
                    sha256,
                    filename,
                } => {
                    out.push_str(&format!("  - url: {}\n", yaml_scalar(url)));
                    out.push_str(&format!("    provider: {}\n", yaml_scalar(provider)));
                    out.push_str(&format!("    sha256: {}\n", yaml_scalar(sha256)));
                    if let Some(f) = filename {
                        out.push_str(&format!("    filename: {}\n", yaml_scalar(f)));
                    }
                }
            }
        }
        out.push('\n');
    }

    if !snap.datapacks.is_empty() {
        out.push_str("datapacks:\n");
        for dp in &snap.datapacks {
            match dp {
                DatapackEntry::Registry {
                    id,
                    provider,
                    version,
                } => {
                    out.push_str(&format!("  - id: {}\n", yaml_scalar(id)));
                    out.push_str(&format!("    provider: {}\n", yaml_scalar(provider)));
                    out.push_str(&format!("    version: {}\n", yaml_scalar(version)));
                }
                DatapackEntry::Url {
                    url,
                    provider,
                    sha256,
                    filename,
                } => {
                    out.push_str(&format!("  - url: {}\n", yaml_scalar(url)));
                    out.push_str(&format!("    provider: {}\n", yaml_scalar(provider)));
                    out.push_str(&format!("    sha256: {}\n", yaml_scalar(sha256)));
                    if let Some(f) = filename {
                        out.push_str(&format!("    filename: {}\n", yaml_scalar(f)));
                    }
                }
                DatapackEntry::VanillaTweaks {
                    provider,
                    packs,
                    version,
                } => {
                    out.push_str(&format!("  - provider: {}\n", yaml_scalar(provider)));
                    if let Some(v) = version {
                        out.push_str(&format!("    version: {}\n", yaml_scalar(v)));
                    }
                    out.push_str("    packs:\n");
                    for (category, names) in packs {
                        out.push_str(&format!("      {}:\n", yaml_scalar(category)));
                        for n in names {
                            out.push_str(&format!("        - {}\n", yaml_scalar(n)));
                        }
                    }
                }
            }
        }
        out.push('\n');
    }

    out.push_str("config:\n");
    out.push_str("  server.properties:\n");
    for (k, v) in snap.config.server_properties.iter() {
        let key = k.as_str().unwrap_or_default();
        out.push_str(&format!("    {}: {}\n", yaml_scalar(key), render_value(v)));
    }
    if !snap.config.files.is_empty() {
        out.push_str("  files:\n");
        for f in &snap.config.files {
            out.push_str(&format!(
                "    - {{ src: {}, dst: {} }}\n",
                yaml_scalar(&f.src),
                yaml_scalar(&f.dst)
            ));
        }
    }

    out
}

#[allow(clippy::too_many_arguments)]
fn build_snap(
    name: &str,
    description: &str,
    minecraft: &str,
    loader: &str,
    location: Option<&str>,
    java: u32,
    memory: &str,
    mods: &[ModEntry],
    server_properties: &Mapping,
    eula: bool,
) -> Snap {
    Snap {
        schema: 1,
        eula,
        server: Server {
            name: name.to_string(),
            description: if description.trim().is_empty() {
                None
            } else {
                Some(description.to_string())
            },
            minecraft: minecraft.to_string(),
            loader: Loader {
                kind: loader.to_string(),
                version: None,
                installer: None,
            },
            location: location.map(|s| s.to_string()),
        },
        runtime: Runtime {
            java: Some(java),
            memory: Some(memory.to_string()),
            flags: vec![],
        },
        mods: mods.to_vec(),
        datapacks: vec![],
        config: ConfigSection {
            server_properties: server_properties.clone(),
            files: vec![],
        },
    }
}

fn render_value(v: &Value) -> String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => yaml_scalar(s),
        Value::Null => "null".to_string(),
        other => serde_yml::to_string(other)
            .unwrap_or_default()
            .trim_end()
            .to_string(),
    }
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
    if existing.lines().any(|l| l.trim() == ".mc-snap")
        || existing.lines().any(|l| l.trim() == ".mc-snap/")
    {
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
fn props_with(motd: &str, max_players: u32) -> Mapping {
    let mut m = Mapping::new();
    m.insert(Value::String("motd".into()), Value::String(motd.into()));
    m.insert(
        Value::String("max-players".into()),
        Value::Number((max_players as i64).into()),
    );
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yml::Snap;

    fn r(
        name: &str,
        desc: &str,
        mc: &str,
        loader: &str,
        location: Option<&str>,
        java: u32,
        mem: &str,
        mods: &[ModEntry],
        props: &Mapping,
        eula: bool,
    ) -> String {
        let snap = build_snap(
            name, desc, mc, loader, location, java, mem, mods, props, eula,
        );
        render_yml(&snap)
    }

    #[test]
    fn rendered_yml_parses_and_round_trips() {
        let mods = vec![ModEntry::Registry {
            id: "fabric-api".into(),
            provider: "modrinth".into(),
            version: "latest".into(),
        }];
        let props = props_with("welcome!", 20);
        let body = r(
            "grimwald",
            "the grimwald smp",
            "26.1.2",
            "fabric",
            None,
            26,
            "4G",
            &mods,
            &props,
            true,
        );
        let snap = Snap::from_str(&body).unwrap();
        assert_eq!(snap.server.name, "grimwald");
        assert_eq!(snap.server.minecraft, "26.1.2");
        assert_eq!(snap.server.loader.kind, "fabric");
        assert_eq!(snap.runtime.java, Some(26));
        assert_eq!(snap.mods.len(), 1);
        assert!(snap.eula);
        assert!(snap.server.location.is_none());
    }

    #[test]
    fn vanilla_without_mods_has_empty_mods() {
        let props = props_with("hi", 10);
        let body = r(
            "vanilla-svr",
            "",
            "26.1.2",
            "vanilla",
            None,
            26,
            "2G",
            &[],
            &props,
            false,
        );
        let snap = Snap::from_str(&body).unwrap();
        assert_eq!(snap.server.loader.kind, "vanilla");
        assert!(snap.mods.is_empty());
        assert!(!snap.eula);
    }

    #[test]
    fn quotes_values_with_special_chars() {
        let mut props = Mapping::new();
        props.insert(
            Value::String("motd".into()),
            Value::String("say \"hi\"".into()),
        );
        props.insert(
            Value::String("max-players".into()),
            Value::Number(20i64.into()),
        );
        let body = r(
            "weird:name",
            "has # hash",
            "26.1.2",
            "fabric",
            None,
            26,
            "4G",
            &[],
            &props,
            false,
        );
        let snap = Snap::from_str(&body).unwrap();
        assert_eq!(snap.server.name, "weird:name");
    }

    #[test]
    fn renders_location_when_present() {
        let props = props_with("hi", 20);
        let body = r(
            "s",
            "",
            "26.1.2",
            "vanilla",
            Some("server"),
            26,
            "2G",
            &[],
            &props,
            true,
        );
        let snap = Snap::from_str(&body).unwrap();
        assert_eq!(snap.server.location.as_deref(), Some("server"));
    }

    #[test]
    fn omits_location_when_none() {
        let props = props_with("hi", 20);
        let body = r(
            "s",
            "",
            "26.1.2",
            "vanilla",
            None,
            26,
            "2G",
            &[],
            &props,
            true,
        );
        assert!(!body.contains("location:"));
        let snap = Snap::from_str(&body).unwrap();
        assert!(snap.server.location.is_none());
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

    #[test]
    fn renders_url_mod_entry() {
        let mods = vec![ModEntry::Url {
            url: "https://example.com/x.jar".into(),
            provider: "url".into(),
            sha256: "0".repeat(64),
            filename: Some("x.jar".into()),
        }];
        let props = props_with("hi", 20);
        let body = r(
            "s", "", "26.1.2", "vanilla", None, 26, "2G", &mods, &props, false,
        );
        let snap = Snap::from_str(&body).unwrap();
        assert_eq!(snap.mods.len(), 1);
    }

    #[test]
    fn server_properties_preserves_numbers_as_ints() {
        let mut props = Mapping::new();
        props.insert(
            Value::String("max-players".into()),
            Value::Number(20i64.into()),
        );
        props.insert(Value::String("online-mode".into()), Value::Bool(true));
        props.insert(Value::String("motd".into()), Value::String("hello".into()));
        let body = r(
            "s",
            "",
            "26.1.2",
            "vanilla",
            None,
            26,
            "2G",
            &[],
            &props,
            false,
        );
        let snap = Snap::from_str(&body).unwrap();
        let saved = &snap.config.server_properties;
        assert_eq!(
            saved[Value::String("max-players".into())],
            Value::Number(20i64.into())
        );
        assert_eq!(
            saved[Value::String("online-mode".into())],
            Value::Bool(true)
        );
    }

    #[test]
    fn renders_config_files_block() {
        let props = props_with("hi", 20);
        let mut snap = build_snap(
            "s",
            "",
            "26.1.2",
            "vanilla",
            None,
            26,
            "2G",
            &[],
            &props,
            false,
        );
        snap.config.files = vec![crate::yml::FileRef {
            src: "configs/chunky.toml".into(),
            dst: "config/chunky.toml".into(),
        }];
        let body = render_yml(&snap);
        assert!(body.contains("files:"));
        let parsed = Snap::from_str(&body).unwrap();
        assert_eq!(parsed.config.files.len(), 1);
        assert_eq!(parsed.config.files[0].src, "configs/chunky.toml");
        assert_eq!(parsed.config.files[0].dst, "config/chunky.toml");
    }

    #[test]
    fn renders_and_round_trips_datapacks() {
        use crate::yml::DatapackEntry;
        use std::collections::BTreeMap;

        let props = props_with("hi", 20);
        let mut snap = build_snap(
            "s",
            "",
            "26.1.2",
            "fabric",
            None,
            26,
            "2G",
            &[],
            &props,
            true,
        );
        let mut packs = BTreeMap::new();
        packs.insert("survival".to_string(), vec!["graves".to_string()]);
        snap.datapacks = vec![
            DatapackEntry::Registry {
                id: "terralith".into(),
                provider: "modrinth".into(),
                version: "latest".into(),
            },
            DatapackEntry::VanillaTweaks {
                provider: "vanillatweaks".into(),
                packs,
                version: Some("1.21".into()),
            },
            DatapackEntry::Url {
                url: "https://example.com/p.zip".into(),
                provider: "url".into(),
                sha256: "0".repeat(64),
                filename: Some("p.zip".into()),
            },
        ];
        let body = render_yml(&snap);
        assert!(body.contains("datapacks:"));
        let parsed = Snap::from_str(&body).unwrap();
        assert_eq!(parsed.datapacks, snap.datapacks);
    }

    #[test]
    fn omits_datapacks_block_when_empty() {
        let props = props_with("hi", 20);
        let body = r(
            "s",
            "",
            "26.1.2",
            "vanilla",
            None,
            26,
            "2G",
            &[],
            &props,
            true,
        );
        assert!(!body.contains("datapacks:"));
    }

    #[test]
    fn round_trips_loader_version_and_flags() {
        let props = props_with("hi", 20);
        let mut snap = build_snap(
            "s",
            "",
            "26.1.2",
            "fabric",
            None,
            26,
            "2G",
            &[],
            &props,
            true,
        );
        snap.server.loader.version = Some("0.16.9".into());
        snap.runtime.flags = vec!["-XX:+UseG1GC".into(), "-XX:+ParallelRefProcEnabled".into()];
        let body = render_yml(&snap);
        let parsed = Snap::from_str(&body).unwrap();
        assert_eq!(parsed.server.loader.version.as_deref(), Some("0.16.9"));
        assert_eq!(parsed.runtime.flags.len(), 2);
    }
}
