use mc_snap_core::yml::Loader;
use mc_snap_core::{LoaderSpec, ServerLoader};
use mc_snap_loaders::vanilla::Vanilla;
use serde_json::json;
use sha1::{Digest, Sha1};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sha1_hex(bytes: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

fn spec() -> LoaderSpec {
    LoaderSpec(Loader { kind: "vanilla".into(), version: None, installer: None })
}

#[tokio::test]
async fn resolves_known_version() {
    let server = MockServer::start().await;

    let jar_bytes = b"FAKE_SERVER_JAR_CONTENT".to_vec();
    let jar_sha1 = sha1_hex(&jar_bytes);

    Mock::given(method("GET"))
        .and(path("/manifest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "versions": [
                { "id": "26.1.2", "url": format!("{}/version/26.1.2.json", server.uri()) },
                { "id": "1.20.4", "url": format!("{}/version/1.20.4.json", server.uri()) }
            ]
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/version/26.1.2.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "downloads": {
                "server": {
                    "url": format!("{}/server.jar", server.uri()),
                    "sha1": jar_sha1,
                    "size": jar_bytes.len()
                }
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/server.jar"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(jar_bytes.clone()))
        .mount(&server)
        .await;

    let v = Vanilla::with_manifest_url(format!("{}/manifest", server.uri()));
    let r = v.resolve("26.1.2", &spec()).await.unwrap();
    assert_eq!(r.kind, "vanilla");
    assert_eq!(r.minecraft, "26.1.2");
    assert_eq!(r.launch_jar, "server.jar");
    assert_eq!(r.server_jar_url, format!("{}/server.jar", server.uri()));
    assert_eq!(r.server_jar_sha256, mc_snap_core::cache::sha256_hex(&jar_bytes));
}

#[tokio::test]
async fn rejects_unknown_version() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/manifest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "versions": [
                { "id": "26.1.2", "url": "ignored" }
            ]
        })))
        .mount(&server)
        .await;

    let v = Vanilla::with_manifest_url(format!("{}/manifest", server.uri()));
    let err = v.resolve("9.9.9", &spec()).await.unwrap_err();
    assert!(err.to_string().contains("unknown minecraft version"));
}
