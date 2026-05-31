use mc_snap::providers::vanillatweaks::VanillaTweaks;
use serde_json::json;
use std::collections::BTreeMap;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn packs() -> BTreeMap<String, Vec<String>> {
    let mut m = BTreeMap::new();
    m.insert(
        "survival".to_string(),
        vec!["graves".to_string(), "afk_display".to_string()],
    );
    m
}

#[tokio::test]
async fn generates_and_downloads_bundle() {
    let server = MockServer::start().await;
    let zip = b"VT_DATAPACK_ZIP".to_vec();
    let sha256 = mc_snap::cache::sha256_hex(&zip);

    Mock::given(method("POST"))
        .and(path("/assets/server/zipdatapacks.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "link": "/download/datapacks/bundle-abc123.zip"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download/datapacks/bundle-abc123.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip))
        .mount(&server)
        .await;

    let vt = VanillaTweaks::with_base(server.uri());
    let r = vt.resolve("1.21", &packs()).await.unwrap();

    assert_eq!(r.provider, "vanillatweaks");
    assert_eq!(r.version, "1.21");
    assert_eq!(r.filename, "bundle-abc123.zip");
    assert_eq!(r.sha256, sha256);
    assert!(r.url.ends_with("/download/datapacks/bundle-abc123.zip"));
}

#[tokio::test]
async fn absolute_link_is_used_verbatim() {
    let server = MockServer::start().await;
    let zip = b"ABS_LINK_ZIP".to_vec();

    Mock::given(method("POST"))
        .and(path("/assets/server/zipdatapacks.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "link": format!("{}/files/dp.zip", server.uri())
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/files/dp.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip))
        .mount(&server)
        .await;

    let vt = VanillaTweaks::with_base(server.uri());
    let r = vt.resolve("1.21", &packs()).await.unwrap();
    assert_eq!(r.filename, "dp.zip");
}

#[tokio::test]
async fn errors_on_failed_status() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/assets/server/zipdatapacks.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "error",
            "message": "unknown pack"
        })))
        .mount(&server)
        .await;

    let vt = VanillaTweaks::with_base(server.uri());
    let err = vt.resolve("1.21", &packs()).await.unwrap_err();
    assert!(err.to_string().contains("unknown pack"), "got: {err}");
}

#[tokio::test]
async fn errors_on_empty_selection() {
    let vt = VanillaTweaks::with_base("http://unused.invalid");
    let empty: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let err = vt.resolve("1.21", &empty).await.unwrap_err();
    assert!(err.to_string().contains("empty"), "got: {err}");
}
