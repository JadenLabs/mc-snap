use mc_snap::providers::curseforge::CurseForge;
use mc_snap::yml::ModEntry;
use mc_snap::{ModProvider, ModSpec, ResolveEnv};
use serde_json::json;
use sha1::Digest as _;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sha1_hex(b: &[u8]) -> String {
    let mut h = sha1::Sha1::new();
    h.update(b);
    hex::encode(h.finalize())
}

fn env(loader: &str) -> ResolveEnv {
    ResolveEnv {
        minecraft: "1.21.1".into(),
        loader_kind: loader.into(),
        loader_version: None,
    }
}

fn registry_spec(id: &str, version: &str) -> ModSpec {
    ModSpec(ModEntry::Registry {
        id: id.into(),
        provider: "curseforge".into(),
        version: version.into(),
    })
}

async fn mock_file(server: &MockServer, route: &str, bytes: Vec<u8>) {
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
        .mount(server)
        .await;
}

#[tokio::test]
async fn resolves_latest_by_numeric_id() {
    let server = MockServer::start().await;
    let jar = b"JEI_BYTES".to_vec();
    let sha1 = sha1_hex(&jar);
    let sha256 = mc_snap::cache::sha256_hex(&jar);
    let file_url = format!("{}/files/jei.jar", server.uri());

    Mock::given(method("GET"))
        .and(path("/mods/238222/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{
                "id": 6001,
                "displayName": "JEI 1.0",
                "fileName": "jei-1.0.jar",
                "downloadUrl": file_url,
                "hashes": [{"value": sha1, "algo": 1}],
                "gameVersions": ["1.21.1", "Fabric"],
                "fileDate": "2024-09-01T00:00:00Z"
            }]
        })))
        .mount(&server)
        .await;
    mock_file(&server, "/files/jei.jar", jar).await;

    let p = CurseForge::with_base(server.uri());
    let r = p
        .resolve(&registry_spec("238222", "latest"), &env("fabric"))
        .await
        .unwrap();
    assert_eq!(r.provider, "curseforge");
    assert_eq!(r.version, "6001");
    assert_eq!(r.filename, "jei-1.0.jar");
    assert_eq!(r.sha256, sha256);
}

#[tokio::test]
async fn resolves_slug_via_search() {
    let server = MockServer::start().await;
    let jar = b"SLUG_BYTES".to_vec();
    let file_url = format!("{}/files/x.jar", server.uri());

    Mock::given(method("GET"))
        .and(path("/mods/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"id": 99999}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/mods/99999/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{
                "id": 7,
                "displayName": "v2",
                "fileName": "x.jar",
                "downloadUrl": file_url,
                "hashes": [],
                "gameVersions": ["1.21.1", "Fabric"]
            }]
        })))
        .mount(&server)
        .await;
    mock_file(&server, "/files/x.jar", jar).await;

    let p = CurseForge::with_base(server.uri());
    let r = p
        .resolve(&registry_spec("jade", "latest"), &env("fabric"))
        .await
        .unwrap();
    assert_eq!(r.version, "7");
    assert_eq!(r.filename, "x.jar");
}

#[tokio::test]
async fn resolves_pinned_file_id() {
    let server = MockServer::start().await;
    let jar_v2 = b"V2".to_vec();
    let v2_url = format!("{}/files/v2.jar", server.uri());

    Mock::given(method("GET"))
        .and(path("/mods/1/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [
                {"id": 200, "displayName": "newer", "fileName": "v3.jar",
                 "downloadUrl": format!("{}/files/v3.jar", server.uri()),
                 "hashes": [], "gameVersions": ["1.21.1", "Fabric"]},
                {"id": 100, "displayName": "older", "fileName": "v2.jar",
                 "downloadUrl": v2_url,
                 "hashes": [], "gameVersions": ["1.21.1", "Fabric"]}
            ]
        })))
        .mount(&server)
        .await;
    mock_file(&server, "/files/v2.jar", jar_v2).await;

    let p = CurseForge::with_base(server.uri());
    let r = p
        .resolve(&registry_spec("1", "100"), &env("fabric"))
        .await
        .unwrap();
    assert_eq!(r.filename, "v2.jar");
}

#[tokio::test]
async fn detects_sha1_mismatch() {
    let server = MockServer::start().await;
    let jar = b"REAL".to_vec();
    let file_url = format!("{}/files/m.jar", server.uri());

    Mock::given(method("GET"))
        .and(path("/mods/1/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{
                "id": 1, "displayName": "m", "fileName": "m.jar",
                "downloadUrl": file_url,
                "hashes": [{"value": "00".repeat(20), "algo": 1}],
                "gameVersions": ["1.21.1", "Fabric"]
            }]
        })))
        .mount(&server)
        .await;
    mock_file(&server, "/files/m.jar", jar).await;

    let p = CurseForge::with_base(server.uri());
    let err = p
        .resolve(&registry_spec("1", "latest"), &env("fabric"))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("sha1 mismatch"), "got: {err}");
}

#[tokio::test]
async fn datapack_mode_ignores_loader_filter() {
    let server = MockServer::start().await;
    let zip = b"DATAPACK_ZIP".to_vec();
    let url = format!("{}/files/dp.zip", server.uri());

    Mock::given(method("GET"))
        .and(path_regex(r"^/mods/\d+/files$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{
                "id": 50, "displayName": "pack", "fileName": "pack.zip",
                "downloadUrl": url,
                "hashes": [],
                "gameVersions": ["1.21.1"]
            }]
        })))
        .mount(&server)
        .await;
    mock_file(&server, "/files/dp.zip", zip).await;

    let p = CurseForge::with_base(server.uri());
    let r = p
        .resolve(&registry_spec("5", "latest"), &env("datapack"))
        .await
        .unwrap();
    assert_eq!(r.filename, "pack.zip");
}

#[tokio::test]
async fn missing_api_key_errors() {
    // `new()` reads the env; clear it so we exercise the no-key path.
    std::env::remove_var("CURSEFORGE_API_KEY");
    std::env::remove_var("CF_API_KEY");
    let p = CurseForge::new();
    let err = p
        .resolve(&registry_spec("1", "latest"), &env("fabric"))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("API key"), "got: {err}");
}

#[tokio::test]
async fn list_versions_separates_loaders() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mods/1/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{
                "id": 11, "displayName": "v", "fileName": "v.jar",
                "downloadUrl": "u", "hashes": [],
                "gameVersions": ["1.21.1", "1.21", "Fabric"],
                "fileDate": "2024-01-01T00:00:00Z"
            }]
        })))
        .mount(&server)
        .await;

    let p = CurseForge::with_base(server.uri());
    let versions = p
        .list_versions(&registry_spec("1", "latest"), &env("fabric"))
        .await
        .unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].version_number, "11");
    assert_eq!(versions[0].game_versions, vec!["1.21.1", "1.21"]);
    assert_eq!(versions[0].loaders, vec!["fabric"]);
    assert_eq!(
        versions[0].date_published.as_deref(),
        Some("2024-01-01T00:00:00Z")
    );
}
