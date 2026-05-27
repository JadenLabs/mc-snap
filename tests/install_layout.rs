use mc_snap::cache::sha256_hex;
use mc_snap::lock::{Lock, LockLoader};
use mc_snap::orchestrate;
use mc_snap::paths::ProjectLayout;
use mc_snap::yml::{ConfigSection, Loader, ModEntry, Server, Snap};
use serde_yml::{Mapping, Value};
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn snap_with_location(loc: Option<&str>) -> Snap {
    let mut props = Mapping::new();
    props.insert(Value::String("motd".into()), Value::String("hi".into()));
    Snap {
        schema: 1,
        eula: true,
        server: Server {
            name: "layout-test".into(),
            description: None,
            minecraft: "26.1.2".into(),
            loader: Loader {
                kind: "vanilla".into(),
                version: None,
                installer: None,
            },
            location: loc.map(|s| s.to_string()),
        },
        runtime: Default::default(),
        mods: Vec::<ModEntry>::new(),
        config: ConfigSection {
            server_properties: props,
            files: vec![],
        },
    }
}

async fn mock_server_jar(server: &MockServer, bytes: &[u8]) -> (String, String) {
    Mock::given(method("GET"))
        .and(path("/server.jar"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes.to_vec()))
        .mount(server)
        .await;
    let url = format!("{}/server.jar", server.uri());
    let sha = sha256_hex(bytes);
    (url, sha)
}

fn lock_for(url: String, sha: String) -> Lock {
    Lock {
        schema: 1,
        yml_hash: "a".repeat(64),
        loader: LockLoader {
            kind: "vanilla".into(),
            minecraft: "26.1.2".into(),
            loader_version: None,
            installer_version: None,
            server_jar_url: url,
            server_jar_sha256: sha,
            extra: vec![],
        },
        mods: vec![],
        jdk: None,
    }
}

#[tokio::test]
async fn materialize_surface_level_puts_server_at_root() {
    let mock = MockServer::start().await;
    let bytes = b"FAKE_VANILLA_JAR".to_vec();
    let (url, sha) = mock_server_jar(&mock, &bytes).await;

    let td = TempDir::new().unwrap();
    let layout = ProjectLayout::at(td.path().to_path_buf());
    std::fs::create_dir_all(layout.snap_dir()).unwrap();

    let snap = snap_with_location(None);
    let lock = lock_for(url, sha);

    orchestrate::materialize(&layout, &snap, &lock).await.unwrap();

    assert!(td.path().join("server.jar").is_file(), "server.jar at root");
    assert!(td.path().join("eula.txt").is_file(), "eula.txt at root");
    assert!(
        td.path().join("server.properties").is_file(),
        "server.properties at root"
    );
    assert!(td.path().join("mods").is_dir(), "mods/ at root");

    assert!(
        td.path().join(".mc-snap").join("state.json").is_file(),
        "state.json under .mc-snap"
    );
    assert!(
        td.path().join(".mc-snap").join("rcon.secret").is_file(),
        "rcon.secret under .mc-snap"
    );
    assert!(
        !td.path().join(".mc-snap").join("server").exists(),
        "no legacy .mc-snap/server directory"
    );
}

#[tokio::test]
async fn materialize_with_location_puts_server_in_subdir() {
    let mock = MockServer::start().await;
    let bytes = b"FAKE_VANILLA_JAR_SUBDIR".to_vec();
    let (url, sha) = mock_server_jar(&mock, &bytes).await;

    let td = TempDir::new().unwrap();
    let layout = ProjectLayout::at(td.path().to_path_buf());
    std::fs::create_dir_all(layout.snap_dir()).unwrap();

    let snap = snap_with_location(Some("server"));
    let lock = lock_for(url, sha);

    orchestrate::materialize(&layout, &snap, &lock).await.unwrap();

    let sub = td.path().join("server");
    assert!(sub.join("server.jar").is_file(), "server.jar in subdir");
    assert!(sub.join("eula.txt").is_file(), "eula.txt in subdir");
    assert!(sub.join("server.properties").is_file(), "server.properties in subdir");
    assert!(sub.join("mods").is_dir(), "mods/ in subdir");

    assert!(!td.path().join("server.jar").exists(), "no jar at root");
    assert!(
        td.path().join(".mc-snap").join("state.json").is_file(),
        ".mc-snap state still at root"
    );
    assert!(
        td.path().join(".mc-snap").join("rcon.secret").is_file(),
        ".mc-snap rcon secret still at root"
    );
}
