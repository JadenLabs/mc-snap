use anyhow::{bail, Result};
use serde_yml::{Mapping, Value};
use std::collections::BTreeMap;

pub fn render(defaults: BTreeMap<&str, String>, overrides: &Mapping) -> Result<String> {
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
        validate_key(&k)?;
        validate_value(&k, &v)?;
        out.push_str(&k);
        out.push('=');
        out.push_str(&v);
        out.push('\n');
    }
    Ok(out)
}

fn validate_key(k: &str) -> Result<()> {
    if k.is_empty() {
        bail!("server.properties has an empty key");
    }
    if k.contains('\n') || k.contains('\r') || k.contains('=') {
        bail!("server.properties key contains forbidden character: {k:?}");
    }
    Ok(())
}

fn validate_value(k: &str, v: &str) -> Result<()> {
    if v.contains('\n') || v.contains('\r') {
        bail!("server.properties value for `{k}` contains a newline; refusing to write");
    }
    Ok(())
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
    // Bind RCON to loopback by default. Minecraft's RCON is plaintext, so the
    // password travels in the clear; never expose it to other hosts.
    m.insert("rcon.ip", "127.0.0.1".into());
    m.insert("broadcast-rcon-to-ops", "false".into());
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_newline_in_value() {
        let mut m = Mapping::new();
        m.insert(
            Value::String("motd".into()),
            Value::String("hi\nenable-rcon=false".into()),
        );
        let err = render(default_properties(), &m).unwrap_err();
        assert!(err.to_string().contains("newline"));
    }

    #[test]
    fn rejects_equals_in_key() {
        let mut m = Mapping::new();
        m.insert(Value::String("foo=bar".into()), Value::String("v".into()));
        assert!(render(default_properties(), &m).is_err());
    }

    #[test]
    fn writes_default_rcon_ip_loopback() {
        let body = render(default_properties(), &Mapping::new()).unwrap();
        assert!(body.contains("rcon.ip=127.0.0.1\n"));
    }
}
