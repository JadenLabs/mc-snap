use mc_snap_core::yml::Loader;
use mc_snap_core::{LoaderSpec, ServerLoader};
use mc_snap_loaders::fabric::Fabric;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn spec(loader: Option<&str>, installer: Option<&str>) -> LoaderSpec {
    LoaderSpec(Loader {
        kind: "fabric".into(),
        version: loader.map(String::from),
        installer: installer.map(String::from),
    })
}

#[tokio::test]
async fn resolves_with_pinned_versions() {
    let server = MockServer::start().await;
    let jar_bytes = b"FABRIC_LAUNCHER_JAR".to_vec();

    Mock::given(method("GET"))
        .and(path("/versions/loader/26.1.2/0.16.9/1.0.1/server/jar"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(jar_bytes.clone()))
        .mount(&server)
        .await;

    let f = Fabric::with_base(server.uri());
    let r = f.resolve("26.1.2", &spec(Some("0.16.9"), Some("1.0.1"))).await.unwrap();
    assert_eq!(r.loader_version.as_deref(), Some("0.16.9"));
    assert_eq!(r.installer_version.as_deref(), Some("1.0.1"));
    assert_eq!(r.launch_jar, "fabric-server-launch.jar");
    assert_eq!(r.server_jar_sha256, mc_snap_core::cache::sha256_hex(&jar_bytes));
}

#[tokio::test]
async fn discovers_latest_when_unpinned() {
    let server = MockServer::start().await;
    let jar_bytes = b"X".to_vec();

    Mock::given(method("GET"))
        .and(path("/versions/loader"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "version": "0.17.0-beta", "stable": false },
            { "version": "0.16.9", "stable": true }
        ])))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/versions/installer"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "version": "2.0.0-beta", "stable": false },
            { "version": "1.0.1", "stable": true }
        ])))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/versions/loader/26.1.2/0.16.9/1.0.1/server/jar"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(jar_bytes.clone()))
        .mount(&server)
        .await;

    let f = Fabric::with_base(server.uri());
    let r = f.resolve("26.1.2", &spec(None, None)).await.unwrap();
    assert_eq!(r.loader_version.as_deref(), Some("0.16.9"));
    assert_eq!(r.installer_version.as_deref(), Some("1.0.1"));
}
