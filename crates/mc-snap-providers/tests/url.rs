use mc_snap_core::yml::ModEntry;
use mc_snap_core::{ModProvider, ModSpec, ResolveEnv};
use mc_snap_providers::url::UrlProvider;

const SHA: &str = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";

fn env() -> ResolveEnv {
    ResolveEnv {
        minecraft: "1.21.4".into(),
        loader_kind: "fabric".into(),
        loader_version: None,
    }
}

#[tokio::test]
async fn derives_filename_from_url() {
    let spec = ModSpec(ModEntry::Url {
        url: "https://example.com/releases/v1/mymod-1.0.jar".into(),
        provider: "url".into(),
        sha256: SHA.into(),
        filename: None,
    });
    let r = UrlProvider::new().resolve(&spec, &env()).await.unwrap();
    assert_eq!(r.filename, "mymod-1.0.jar");
    assert_eq!(r.id, "mymod-1.0");
}

#[tokio::test]
async fn strips_query_string_from_filename() {
    let spec = ModSpec(ModEntry::Url {
        url: "https://example.com/file.jar?token=abc".into(),
        provider: "url".into(),
        sha256: SHA.into(),
        filename: None,
    });
    let r = UrlProvider::new().resolve(&spec, &env()).await.unwrap();
    assert_eq!(r.filename, "file.jar");
}

#[tokio::test]
async fn explicit_filename_wins() {
    let spec = ModSpec(ModEntry::Url {
        url: "https://example.com/x.jar".into(),
        provider: "url".into(),
        sha256: SHA.into(),
        filename: Some("custom-name.jar".into()),
    });
    let r = UrlProvider::new().resolve(&spec, &env()).await.unwrap();
    assert_eq!(r.filename, "custom-name.jar");
}
