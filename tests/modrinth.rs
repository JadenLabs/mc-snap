use mc_snap::yml::ModEntry;
use mc_snap::{ModProvider, ModSpec, ResolveEnv};
use mc_snap::providers::modrinth::Modrinth;
use serde_json::json;
use sha2::Digest;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sha512_hex(b: &[u8]) -> String {
    let mut h = sha2::Sha512::new();
    h.update(b);
    hex::encode(h.finalize())
}

fn env() -> ResolveEnv {
    ResolveEnv {
        minecraft: "26.1.2".into(),
        loader_kind: "fabric".into(),
        loader_version: None,
    }
}

fn registry_spec(version: &str) -> ModSpec {
    ModSpec(ModEntry::Registry {
        id: "fabric-api".into(),
        provider: "modrinth".into(),
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
async fn resolves_latest_version() {
    let server = MockServer::start().await;
    let jar = b"FABRIC_API_BYTES".to_vec();
    let sha512 = sha512_hex(&jar);
    let sha256 = mc_snap::cache::sha256_hex(&jar);
    let file_url = format!("{}/files/fabric-api.jar", server.uri());

    Mock::given(method("GET"))
        .and(path("/project/fabric-api/version"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": "abc",
                "version_number": "0.140.0+26.1.2",
                "game_versions": ["26.1.2"],
                "loaders": ["fabric"],
                "files": [{
                    "url": file_url,
                    "filename": "fabric-api-0.110.0.jar",
                    "hashes": {"sha512": sha512, "sha1": "deadbeef"},
                    "primary": true
                }]
            }
        ])))
        .mount(&server)
        .await;
    mock_file(&server, "/files/fabric-api.jar", jar).await;

    let p = Modrinth::with_base(server.uri());
    let r = p.resolve(&registry_spec("latest"), &env()).await.unwrap();
    assert_eq!(r.version, "0.140.0+26.1.2");
    assert_eq!(r.filename, "fabric-api-0.110.0.jar");
    assert_eq!(r.sha256, sha256);
}

#[tokio::test]
async fn resolves_pinned_version() {
    let server = MockServer::start().await;
    let jar_v2 = b"V2_BYTES".to_vec();
    let sha512_v2 = sha512_hex(&jar_v2);
    let file_v2_url = format!("{}/files/v2.jar", server.uri());

    Mock::given(method("GET"))
        .and(path("/project/fabric-api/version"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": "v1", "version_number": "0.111.0",
                "game_versions": ["26.1.2"], "loaders": ["fabric"],
                "files": [{
                    "url": format!("{}/files/v1.jar", server.uri()),
                    "filename": "f1.jar",
                    "hashes": {"sha512": "x".repeat(128)}, "primary": true
                }]
            },
            {
                "id": "v2", "version_number": "0.140.0+26.1.2",
                "game_versions": ["26.1.2"], "loaders": ["fabric"],
                "files": [{
                    "url": file_v2_url,
                    "filename": "f2.jar",
                    "hashes": {"sha512": sha512_v2}, "primary": true
                }]
            }
        ])))
        .mount(&server)
        .await;
    mock_file(&server, "/files/v2.jar", jar_v2.clone()).await;

    let p = Modrinth::with_base(server.uri());
    let r = p.resolve(&registry_spec("0.140.0+26.1.2"), &env()).await.unwrap();
    assert_eq!(r.filename, "f2.jar");
    assert_eq!(r.sha256, mc_snap::cache::sha256_hex(&jar_v2));
}

#[tokio::test]
async fn rejects_unsupported_minecraft() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/project/fabric-api/version"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": "abc", "version_number": "0.110.0",
                "game_versions": ["1.20.4"], "loaders": ["fabric"],
                "files": [{
                    "url": "u", "filename": "f.jar",
                    "hashes": {"sha512": "x".repeat(128)}, "primary": true
                }]
            }
        ])))
        .mount(&server)
        .await;

    let p = Modrinth::with_base(server.uri());
    let err = p.resolve(&registry_spec("latest"), &env()).await.unwrap_err();
    assert!(err.to_string().contains("does not support minecraft"));
}

#[tokio::test]
async fn list_versions_filters_by_minecraft() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/project/fabric-api/version"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": "v3", "version_number": "0.141.0",
                "game_versions": ["26.1.3"], "loaders": ["fabric"],
                "files": [{
                    "url": "u3", "filename": "f3.jar",
                    "hashes": {"sha512": "a".repeat(128)}, "primary": true
                }],
                "date_published": "2026-05-01T00:00:00Z"
            },
            {
                "id": "v2", "version_number": "0.140.0",
                "game_versions": ["26.1.2"], "loaders": ["fabric"],
                "files": [{
                    "url": "u2", "filename": "f2.jar",
                    "hashes": {"sha512": "b".repeat(128)}, "primary": true
                }]
            }
        ])))
        .mount(&server)
        .await;

    let p = Modrinth::with_base(server.uri());
    let versions = p
        .list_versions(&registry_spec("latest"), &env())
        .await
        .unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].version_number, "0.141.0");
    assert_eq!(versions[0].game_versions, vec!["26.1.3"]);
    assert_eq!(versions[0].loaders, vec!["fabric"]);
    assert_eq!(
        versions[0].date_published.as_deref(),
        Some("2026-05-01T00:00:00Z")
    );
    assert_eq!(versions[1].version_number, "0.140.0");
    assert!(versions[1].date_published.is_none());
}

#[tokio::test]
async fn list_versions_with_empty_minecraft_omits_filter() {
    let server = MockServer::start().await;
    // No `game_versions` query param expected when minecraft is empty.
    Mock::given(method("GET"))
        .and(path("/project/fabric-api/version"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": "any", "version_number": "9.9.9",
                "game_versions": ["26.1.1", "26.1.2", "26.1.3"],
                "loaders": ["fabric"],
                "files": [{
                    "url": "u", "filename": "f.jar",
                    "hashes": {"sha512": "c".repeat(128)}, "primary": true
                }]
            }
        ])))
        .mount(&server)
        .await;

    let mut env_all = env();
    env_all.minecraft = String::new();
    let p = Modrinth::with_base(server.uri());
    let versions = p
        .list_versions(&registry_spec("latest"), &env_all)
        .await
        .unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].game_versions.len(), 3);
}

#[tokio::test]
async fn list_versions_rejects_url_entries() {
    let server = MockServer::start().await;
    let p = Modrinth::with_base(server.uri());
    let spec = ModSpec(ModEntry::Url {
        url: "https://example.com/x.jar".into(),
        provider: "url".into(),
        sha256: "0".repeat(64),
        filename: None,
    });
    let err = p.list_versions(&spec, &env()).await.unwrap_err();
    assert!(err.to_string().contains("cannot list versions"));
}

#[tokio::test]
async fn detects_sha512_mismatch() {
    let server = MockServer::start().await;
    let jar = b"REAL_BYTES".to_vec();
    let wrong_sha = "0".repeat(128);
    let file_url = format!("{}/files/x.jar", server.uri());

    Mock::given(method("GET"))
        .and(path("/project/fabric-api/version"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": "abc", "version_number": "0.110.0",
                "game_versions": ["26.1.2"], "loaders": ["fabric"],
                "files": [{
                    "url": file_url, "filename": "x.jar",
                    "hashes": {"sha512": wrong_sha}, "primary": true
                }]
            }
        ])))
        .mount(&server)
        .await;
    mock_file(&server, "/files/x.jar", jar).await;

    let p = Modrinth::with_base(server.uri());
    let err = p.resolve(&registry_spec("latest"), &env()).await.unwrap_err();
    assert!(err.to_string().contains("sha512 mismatch"));
}
