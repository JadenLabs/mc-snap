# mc-snap - Design

## Context

`mc-snap` is a CLI tool for declarative Minecraft server management - "docker-compose for Minecraft servers". A user writes one YAML file pinning the Minecraft version, loader (Fabric/Vanilla), mods, Java runtime, and configs. The tool resolves and downloads everything, manages the Java process lifecycle, and produces a shareable bundle so others can reproduce the exact same server with one command.

## Decisions locked in

| Area | Choice |
|---|---|
| Language | Rust |
| Scope | Setup **+** lifecycle (start/stop/logs/status) |
| Server types (v1) | Vanilla, Fabric (extensible) |
| Mod providers (v1) | Modrinth, Direct URL / GitHub releases (extensible) |
| Configs | Inline overrides + external file references |
| Bundles | Source bundle (YAML + configs, no jars) |
| Java | Hybrid - prefer system JDK, fall back to auto-download (Adoptium Temurin) |
| CLI name | `mc-snap` |

## Architectural principles

- **Reproducibility first.** A `mc-snap.lock` pins exact versions + SHA-256 hashes for every download, like `Cargo.lock`. Two `mc-snap install` runs on the same lockfile produce byte-identical server directories.
- **Trait-based extension points.** `ServerLoader` and `ModProvider` traits make it cheap to add Paper, Forge, Hangar, CurseForge later without touching core logic.
- **Separation of declared vs generated.** The user owns `mc-snap.yml`, `mc-snap.lock`, and `configs/`. The tool owns `.mc-snap/` (cache, server runtime, pid, logs) and treats it as disposable.
- **Content-addressed cache.** Jars are stored once per SHA-256 under the OS user-data dir (`~/.local/share/mc-snap/cache/` on Linux, `%APPDATA%\mc-snap\mc-snap\data\cache` on Windows) and symlinked (hardlinked on Windows) into each server's mods folder. Multiple servers sharing Sodium download it once.
- **RCON for lifecycle, not stdin piping.** We auto-enable RCON in `server.properties` (unless the user opts out) and use it for `stop` and `console`. This avoids platform-specific stdin/pty wrangling.

## YAML schema

```yaml
schema: 1                    # for future migrations

server:
  name: grimwald
  description: the grimwald smp
  minecraft: 26.1.2          # Minecraft version (26.x is the current series)
  loader:
    type: fabric             # fabric | vanilla
    version: 0.17.4          # loader version (optional - latest stable if omitted)
    installer: 1.1.0         # fabric installer version (optional)

runtime:
  java: 26                   # major version (mc 26.x requires Java 26)
  memory: 4G                 # becomes -Xmx and -Xms
  flags:                     # extra JVM flags appended after memory
    - -XX:+UseG1GC

mods:
  - id: fabric-api
    provider: modrinth
    version: "0.140.0+26.1.2"
  - id: sodium
    provider: modrinth
    version: latest          # resolved + pinned in lockfile
  - url: https://github.com/owner/repo/releases/download/v1.0/mymod.jar
    provider: url
    sha256: abc123...        # required when provider: url

config:
  server.properties:         # inline overrides merged into generated file
    motd: "Welcome to Grimwald"
    max-players: 20
  files:                     # external file references
    - src: configs/sodium-options.json
      dst: config/sodium-options.json
```

**Notes**
- `minecraft:` is the Minecraft version. Pulled out into its own field rather than living next to `loader.type` so loader-specific version fields nest naturally beneath the loader.
- `loader.type` replaces a top-level `type:` so loader-specific version fields nest naturally.
- `provider: url` requires `sha256` - no unpinned remote downloads.
- `eula: true` will be required at the top level on first install (legal).

## Directory layout (per server)

```
my-server/
├── mc-snap.yml              # declared config (user-edited, version-controlled)
├── mc-snap.lock             # pinned versions + hashes (committed)
├── configs/                 # external config files referenced by yml
│   └── sodium-options.json
└── .mc-snap/                # generated, gitignored
    ├── server/              # actual server root (server.jar, mods/, world/, logs/)
    ├── snapshots/           # pre-update yml + lock + configs, used by `revert`
    ├── state.json           # last-applied lockfile hash, install status
    ├── pid                  # running server pid + start-token (for pid-reuse detection)
    ├── rcon.secret          # auto-generated RCON password (mode 0600 on Unix)
    └── .lock                # advisory lock held by mutating commands (install/update/start/...)
```

Global cache (shared across servers, resolved via `directories::ProjectDirs`):
`~/.local/share/mc-snap/{cache,jdks}/` on Linux,
`~/Library/Application Support/mc-snap/{cache,jdks}/` on macOS,
`%APPDATA%\mc-snap\mc-snap\data\{cache,jdks}` on Windows.

## Crate layout

Single crate (`mc-snap`) with both a `[[bin]]` and `[lib]`. Modules are split by concern; the library surface exists so the wiremock integration tests in `tests/` can drive provider/loader code paths directly. Trait-based extension points (`ServerLoader`, `ModProvider`) keep the boundaries explicit even without separate crates.

```
mc-snap/
├── Cargo.toml
└── src/
    ├── main.rs              # binary entry; clap subcommands
    ├── lib.rs               # re-exports the modules below for tests + docs
    ├── commands/            # one file per CLI subcommand
    ├── orchestrate.rs       # install/materialize glue (resolver + downloader + properties writer)
    ├── compat.rs            # update/check version comparison + mod-compat queries
    ├── props.rs             # server.properties renderer with key/value validation
    ├── yml.rs lock.rs cache.rs download.rs paths.rs proclock.rs snapshot.rs state.rs traits.rs
    ├── providers/           # ModProvider trait + modrinth, url impls
    ├── loaders/             # ServerLoader trait + vanilla, fabric impls
    └── runtime/             # Java discovery/download, process spawn + pid/start-token tracking, RCON client
```

## Core traits

```rust
// src/loaders/mod.rs
#[async_trait]
pub trait ServerLoader: Send + Sync {
    fn id(&self) -> &'static str;                       // "fabric", "vanilla"
    async fn resolve(&self, mc: &McVersion, spec: &LoaderSpec)
        -> Result<ResolvedLoader>;                       // -> exact versions + download URLs
    async fn install(&self, resolved: &ResolvedLoader, dst: &Path) -> Result<()>;
    fn launch_command(&self, ctx: &LaunchCtx) -> Command;
}

// src/providers/mod.rs
#[async_trait]
pub trait ModProvider: Send + Sync {
    fn id(&self) -> &'static str;                       // "modrinth", "url"
    async fn resolve(&self, spec: &ModSpec, env: &ResolveEnv)
        -> Result<ResolvedMod>;                          // -> url + sha256 + filename
}
```

Downloading is shared infrastructure in the `download` module - providers only resolve, the core fetches with retry/verify/cache.

## Commands

| Command | Purpose |
|---|---|
| `mc-snap init [--non-interactive]` | Interactive scaffold of a new `mc-snap.yml`; `--non-interactive` writes the default template |
| `mc-snap install` | Resolve, download, write `.mc-snap/server/`, update lockfile |
| `mc-snap update --to <ver> [--skip-missing] [--yes] [--loader <ver>]` | Snapshot, then update yml + lockfile to a new Minecraft version. Auto-rolls back on failure |
| `mc-snap revert [<id>] [--list]` | Restore a snapshot (latest by default); `--list` shows them |
| `mc-snap check --to <ver>` | Per-mod compatibility report (no filesystem changes) |
| `mc-snap updatable [--to <ver>]` | Yes/no answer or "newest MC every mod supports" |
| `mc-snap search` | Newer mod versions on the current MC |
| `mc-snap start [--detach]` | Start server (foreground by default) |
| `mc-snap stop` | Graceful stop via RCON `stop` |
| `mc-snap restart` | `stop` (with port-release wait) then `start --detach` |
| `mc-snap status` | Running/stopped, uptime, player count (via RCON) |
| `mc-snap logs [-f]` | Tail `.mc-snap/server/logs/latest.log` |
| `mc-snap console [cmd...]` | One-shot RCON command, or interactive shell with no args |
| `mc-snap pack [-o out.zip]` | Source bundle: `mc-snap.yml` + `mc-snap.lock` + `configs/` (zip, not tarball) |
| `mc-snap unpack <bundle.zip>` | Extract bundle into current dir (path-validated, no zip-slip) |
| `mc-snap validate` | Schema check without network |
| `mc-snap doctor` | Verify Java, network reachability, disk space |

## Process lifecycle

- **Foreground (default):** parent process is the server. The Java process inherits signals; Ctrl-C goes straight to it, and Minecraft's own handler shuts it down cleanly.
- **Detached (`--detach`):** `setsid` on Unix, `DETACHED_PROCESS` on Windows. Pid + a per-OS start-token (process creation time on Windows, `/proc/<pid>/stat` field 22 on Linux) is written to `.mc-snap/pid` so we detect pid reuse after a crash or reboot.
- **Stop:** RCON `stop` command, then poll the pid. On Unix, escalate to SIGTERM and then SIGKILL after grace windows. On Windows there is no SIGTERM equivalent, so we fall through directly to `TerminateProcess`.
- **Restart:** `stop`, then probe the listening port until it's free (Windows releases the socket asynchronously after `TerminateProcess`), then `start --detach`.
- **Logs:** Minecraft already writes `logs/latest.log`. `mc-snap logs` is a tail wrapper, not a re-implementation.

## Java management (hybrid)

1. Probe `JAVA_HOME`, `$PATH` `java`, and platform-typical locations (`/usr/lib/jvm/*`, macOS `/Library/Java/JavaVirtualMachines/*`, common Windows paths).
2. For each candidate, parse `java -version` output for major version.
3. If any matches the required major → use it.
4. Otherwise, download Temurin from `api.adoptium.net` into `~/.mc-snap/jdks/<major>/<os>-<arch>/` and use that. Cache forever.

## Reproducibility / lockfile

`mc-snap.lock` is human-readable TOML. Example sketch:

```toml
schema = 1
yml_hash = "sha256:..."           # mc-snap.yml hash at lock time

[loader]
type = "fabric"
minecraft = "26.1.2"
loader_version = "0.17.4"
installer_version = "1.1.0"
server_jar_url = "..."
server_jar_sha256 = "..."

[[mods]]
id = "fabric-api"
provider = "modrinth"
version = "0.140.0+26.1.2"
filename = "fabric-api-0.140.0+26.1.2.jar"
url = "https://cdn.modrinth.com/..."
sha256 = "..."
```

`install` is a no-op when `state.json.applied_lock_hash == sha256(mc-snap.lock)`.

## Minecraft 26.x support

The 26.x series is the current default. Concretely, supporting it meant:

- **Java 26 baseline.** The `init` scaffold pins `java: 26` and the `start` fallback uses 26 when `runtime.java` is unset. Older system JDKs trigger an Adoptium Temurin 26 download into `~/.local/share/mc-snap/jdks/26/<os>-<arch>/`.
- **Version strings are opaque.** Loader and provider code passes the `minecraft:` value straight through to Mojang's version manifest, Fabric Meta, and Modrinth's `game_versions` filter. The year-based `26.x.x` format works without parser changes because nothing splits on dots.
- **Fabric loader/installer versions.** The defaults committed in DESIGN/README examples (`0.17.4` / `1.1.0`) track the first stable Fabric release for 26.1.2; users can pin or omit (latest stable resolves at install time).

## Implementation notes (resolved during the build)

- **Modrinth API shape.** Modrinth's `/version` payload exposes `hashes.sha512` and `hashes.sha1`, not `sha256`. The provider downloads the file in `resolve`, verifies the upstream sha512, computes our own sha256 for the lockfile, and pre-populates the content cache so the install step's `fetch_into_cache` short-circuits.
- **Fabric launch flags.** Setting `-Dfabric.installer.server.gameJar=server.jar` triggers an NPE inside the Fabric installer when the path's parent is null. Launching the loader jar with no extra flags works: the launcher downloads the Minecraft jar and libraries on first boot.
- **Detached spawn.** On Unix we set a `pre_exec` hook that calls `setsid` (via `nix`) so the child survives the CLI exit. Pid is recorded in `.mc-snap/pid`.
- **EULA.** Required at the top of `mc-snap.yml`; install refuses without it and writes `eula.txt=true` accordingly.
- **Bundles.** `pack` writes a `.zip` (DEFLATE) of `mc-snap.yml` + `mc-snap.lock` + `configs/`, no jars. `unpack` extracts it into the current directory.
- **Windows cache linking.** Symlink on Unix, hardlink on Windows (no admin requirement).

## Security model

- **No unpinned downloads.** Every artifact is hashed. URL mods require a user-supplied `sha256`. Modrinth files are verified against the API's published `sha512` before they hit the cache; the lockfile sha256 is computed from the same verified bytes. The Mojang server jar is verified against the manifest's `sha1`. The Adoptium JDK download is verified against the published sha256 fetched from `/v3/checksum/latest/...` before extraction.
- **Atomic writes.** Cache files, lockfile, yml, snapshots, and pid file are all written via tmp+rename so a crash mid-write can't leave a half-formed file the next run trips over.
- **Zip extraction is path-validated.** `unpack` and the JDK extractor both walk archive entries by hand, rejecting absolute paths, `..` components, and symlink entries; output paths are canonicalized and asserted to stay under the destination.
- **RCON secret hygiene.** 32 bytes from `OsRng`, hex-encoded. Written 0600 on Unix. `rcon.ip` is force-set to `127.0.0.1` on every install - the password is plaintext on the wire so we never let it listen externally.
- **Project advisory lock.** A `fs2` exclusive lock on `.mc-snap/.lock` serializes mutating commands (install / update / start / stop / revert) so two runs can't stomp each other's yml + server dir.
- **Download caps.** The reqwest client has bounded redirects (5) and total/connect timeouts. Streamed downloads abort if they exceed a hard size cap.

## Future scope

- Plugin/datapack support for Fabric (`datapacks:`, `resourcepacks:` keys).
- Additional loaders: Paper, Forge, NeoForge.
- Additional providers: Hangar, CurseForge.

## Verification

What ships in the repo:

1. **Unit tests** colocated with each crate - schema parsing, lockfile round-trip, RCON framing, Java version parsing.
2. **Wiremock integration tests** in `tests/` - exercise Modrinth, URL, Mojang manifest, and Fabric Meta against stubbed HTTP servers.
3. **Live end-to-end** in `scripts/e2e.sh` - builds the binary, scaffolds a Fabric 26.1.2 + fabric-api server under `.dev-servers/e2e/`, runs the full lifecycle (`install` x2, `start --detach`, boot wait, `status`, `console list`, `console say`, `logs`, `stop`), then `pack` -> fresh dir -> `unpack` -> `validate`. Driven by `make e2e`.
