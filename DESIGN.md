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
- **Content-addressed cache.** Jars are stored once per SHA-256 in `~/.mc-snap/cache/` and symlinked (or hardlinked on Windows) into each server's mods folder. Multiple servers sharing Sodium download it once.
- **RCON for lifecycle, not stdin piping.** We auto-enable RCON in `server.properties` (unless the user opts out) and use it for `stop` and `console`. This avoids platform-specific stdin/pty wrangling.

## YAML schema

```yaml
schema: 1                    # for future migrations

server:
  name: grimwald
  description: the grimwald smp
  minecraft: 1.21.4          # Minecraft version
  loader:
    type: fabric             # fabric | vanilla
    version: 0.16.9          # loader version (optional - latest stable if omitted)
    installer: 1.0.1         # fabric installer version (optional)

runtime:
  java: 21                   # major version
  memory: 4G                 # becomes -Xmx and -Xms
  flags:                     # extra JVM flags appended after memory
    - -XX:+UseG1GC

mods:
  - id: fabric-api
    provider: modrinth
    version: "0.110.0+1.21.4"
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
- `minecraft:` is the Minecraft version. The user's first sketch used `version: 26.1.2`, which isn't a real MC version - clarified by giving it its own field.
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
    ├── cache/               # symlinks into the global content-addressed cache
    ├── state.json           # last-applied lockfile hash, install status
    ├── pid                  # running server pid (absent when stopped)
    └── rcon.secret          # auto-generated RCON password
```

Global cache (shared across servers): `~/.mc-snap/{cache,jdks}/`.

## Crate layout

Cargo workspace, modules split by concern. Workspace (not single-crate) because trait-based extension points are central and clean crate boundaries enforce them.

```
mc-snap/
├── Cargo.toml               # workspace
└── crates/
    ├── mc-snap-cli/         # binary; clap subcommands; thin wrapper over core
    ├── mc-snap-core/        # YAML parsing, lockfile, resolver, state machine
    ├── mc-snap-providers/   # ModProvider trait + modrinth, url impls
    ├── mc-snap-loaders/     # ServerLoader trait + vanilla, fabric impls
    └── mc-snap-runtime/     # Java discovery/download, process spawn, RCON client
```

## Core traits

```rust
// crates/mc-snap-loaders/src/lib.rs
#[async_trait]
pub trait ServerLoader: Send + Sync {
    fn id(&self) -> &'static str;                       // "fabric", "vanilla"
    async fn resolve(&self, mc: &McVersion, spec: &LoaderSpec)
        -> Result<ResolvedLoader>;                       // -> exact versions + download URLs
    async fn install(&self, resolved: &ResolvedLoader, dst: &Path) -> Result<()>;
    fn launch_command(&self, ctx: &LaunchCtx) -> Command;
}

// crates/mc-snap-providers/src/lib.rs
#[async_trait]
pub trait ModProvider: Send + Sync {
    fn id(&self) -> &'static str;                       // "modrinth", "url"
    async fn resolve(&self, spec: &ModSpec, env: &ResolveEnv)
        -> Result<ResolvedMod>;                          // -> url + sha256 + filename
}
```

Downloading is shared infrastructure in `mc-snap-core` - providers only resolve, the core fetches with retry/verify/cache.

## Commands

| Command | Purpose |
|---|---|
| `mc-snap init` | Interactive scaffold of a new `mc-snap.yml` |
| `mc-snap install` | Resolve, download, write `.mc-snap/server/`, update lockfile |
| `mc-snap update [mod...]` | Refresh versions, rewrite lockfile (no install) |
| `mc-snap start [--detach]` | Start server (foreground by default) |
| `mc-snap stop` | Graceful stop via RCON `stop` |
| `mc-snap restart` | `stop` then `start --detach` |
| `mc-snap status` | Running/stopped, uptime, player count (via RCON) |
| `mc-snap logs [-f]` | Tail `.mc-snap/server/logs/latest.log` |
| `mc-snap console` | Interactive RCON shell |
| `mc-snap pack [-o out.tar.gz]` | Source bundle: `mc-snap.yml` + `mc-snap.lock` + `configs/` |
| `mc-snap unpack <bundle>` | Extract bundle into current dir |
| `mc-snap validate` | Schema check without network |
| `mc-snap doctor` | Verify Java, network reachability, disk space |

## Process lifecycle

- **Foreground (default):** parent process is the server. Ctrl-C → RCON `stop` → wait → exit.
- **Detached (`--detach`):** double-fork on Unix / `CreateProcess` with `DETACHED_PROCESS` on Windows. Pid written to `.mc-snap/pid`.
- **Stop:** RCON `stop` command, then poll the pid, SIGTERM after a grace window, SIGKILL as last resort.
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
minecraft = "1.21.4"
loader_version = "0.16.9"
installer_version = "1.0.1"
server_jar_url = "..."
server_jar_sha256 = "..."

[[mods]]
id = "fabric-api"
provider = "modrinth"
version = "0.110.0+1.21.4"
filename = "fabric-api-0.110.0+1.21.4.jar"
url = "https://cdn.modrinth.com/..."
sha256 = "..."
```

`install` is a no-op when `state.json.applied_lock_hash == sha256(mc-snap.lock)`.

## Open questions to revisit during implementation

- **EULA.** Require `eula: true` at top of yml; refuse to install without it. Write `eula.txt` accordingly.
- **Plugin/datapack support for Fabric.** Fabric uses mods only; datapacks/resourcepacks can be modeled later under separate keys (`datapacks:`, `resourcepacks:`).
- **Windows.** Test from the start - symlinks need admin or developer mode, so prefer hardlinks for the cache on Windows.

## Verification plan

End-to-end, in order of cheapness:

1. **Unit tests** in each crate - schema parsing, lockfile round-trip, version resolution against recorded API fixtures.
2. **Integration tests** in `crates/mc-snap-cli/tests/` using `wiremock` to fake Modrinth and Adoptium. Run `mc-snap install` against a fixture yml, snapshot the resulting lockfile + directory tree.
3. **Live smoke test** (manual or behind a feature flag): scaffold a Fabric 1.21.4 + Fabric API server, `mc-snap install && mc-snap start --detach`, wait for "Done!" in logs, send `list` via `mc-snap console`, `mc-snap stop`, assert the process is gone.
4. **Bundle round-trip:** `mc-snap pack` → fresh directory → `mc-snap unpack && mc-snap install` → diff the two server trees (ignoring world/logs).
