use serde_yml::{Mapping, Value};
use std::collections::BTreeMap;

pub fn render(defaults: BTreeMap<&str, String>, overrides: &Mapping) -> String {
    let mut merged: BTreeMap<String, String> = defaults
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    for (k, v) in overrides {
        if let Some(key) = k.as_str() {
            merged.insert(key.to_string(), stringify(v));
        }
    }
    let mut out = String::new();
    for (k, v) in merged {
        out.push_str(&k);
        out.push('=');
        out.push_str(&v);
        out.push('\n');
    }
    out
}

fn stringify(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => String::new(),
        other => serde_yml::to_string(other)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

pub fn default_properties() -> BTreeMap<&'static str, String> {
    let mut m = BTreeMap::new();
    m.insert("server-port", "25565".into());
    m.insert("motd", "A mc-snap server".into());
    m.insert("max-players", "20".into());
    m.insert("online-mode", "true".into());
    m.insert("enable-rcon", "true".into());
    m.insert("rcon.port", "25575".into());
    m.insert("broadcast-rcon-to-ops", "false".into());
    m
}
