use mc_snap::cache::sha256_hex;
use mc_snap::lock::{Lock, LockLoader, LockMod};
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
        datapacks: vec![],
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
        datapacks: vec![],
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

    orchestrate::materialize(&layout, &snap, &lock, mc_snap::cache::LinkMode::default())
        .await
        .unwrap();

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

async fn mock_datapack(server: &MockServer, route: &str, bytes: &[u8]) -> (String, String) {
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes.to_vec()))
        .mount(server)
        .await;
    (format!("{}{}", server.uri(), route), sha256_hex(bytes))
}

#[tokio::test]
async fn materialize_installs_datapacks_under_world() {
    let mock = MockServer::start().await;
    let jar = b"FAKE_JAR_DP".to_vec();
    let (url, sha) = mock_server_jar(&mock, &jar).await;
    let dp = b"DATAPACK_BYTES".to_vec();
    let (dp_url, dp_sha) = mock_datapack(&mock, "/files/terralith.zip", &dp).await;

    let td = TempDir::new().unwrap();
    let layout = ProjectLayout::at(td.path().to_path_buf());
    std::fs::create_dir_all(layout.snap_dir()).unwrap();

    let snap = snap_with_location(None);
    let mut lock = lock_for(url, sha);
    lock.datapacks = vec![LockMod {
        id: "terralith".into(),
        provider: "modrinth".into(),
        version: "2.5.0".into(),
        filename: "terralith.zip".into(),
        url: dp_url,
        sha256: dp_sha,
    }];

    orchestrate::materialize(&layout, &snap, &lock, mc_snap::cache::LinkMode::default())
        .await
        .unwrap();

    let installed = td
        .path()
        .join("world")
        .join("datapacks")
        .join("terralith.zip");
    assert!(
        installed.is_file(),
        "datapack installed under world/datapacks"
    );
}

#[tokio::test]
async fn materialize_honors_custom_level_name_for_datapacks() {
    let mock = MockServer::start().await;
    let jar = b"FAKE_JAR_LEVEL".to_vec();
    let (url, sha) = mock_server_jar(&mock, &jar).await;
    let dp = b"DATAPACK_LEVEL".to_vec();
    let (dp_url, dp_sha) = mock_datapack(&mock, "/files/pack.zip", &dp).await;

    let td = TempDir::new().unwrap();
    let layout = ProjectLayout::at(td.path().to_path_buf());
    std::fs::create_dir_all(layout.snap_dir()).unwrap();

    let mut snap = snap_with_location(None);
    snap.config.server_properties.insert(
        Value::String("level-name".into()),
        Value::String("myworld".into()),
    );
    let mut lock = lock_for(url, sha);
    lock.datapacks = vec![LockMod {
        id: "pack".into(),
        provider: "url".into(),
        version: "pinned".into(),
        filename: "pack.zip".into(),
        url: dp_url,
        sha256: dp_sha,
    }];

    orchestrate::materialize(&layout, &snap, &lock, mc_snap::cache::LinkMode::default())
        .await
        .unwrap();

    assert!(
        td.path()
            .join("myworld")
            .join("datapacks")
            .join("pack.zip")
            .is_file(),
        "datapack installed under custom level-name dir"
    );
    assert!(
        !td.path()
            .join("world")
            .join("datapacks")
            .join("pack.zip")
            .exists(),
        "no install under default world dir"
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

    orchestrate::materialize(&layout, &snap, &lock, mc_snap::cache::LinkMode::default())
        .await
        .unwrap();

    let sub = td.path().join("server");
    assert!(sub.join("server.jar").is_file(), "server.jar in subdir");
    assert!(sub.join("eula.txt").is_file(), "eula.txt in subdir");
    assert!(
        sub.join("server.properties").is_file(),
        "server.properties in subdir"
    );
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
