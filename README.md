# mc-snap

Declarative Minecraft server management. Write one YAML file pinning the Minecraft version, loader, mods, Java runtime, and configs; `mc-snap` resolves and downloads everything, manages the server process, and produces a shareable bundle so anyone can reproduce the exact same server with one command.

Think "docker-compose for Minecraft servers".

## Status

Working: Vanilla and Fabric loaders, Modrinth and direct-URL mod providers, install / start / stop / status / logs / console / pack / unpack / validate / doctor / check / updatable / search / update / revert, system Java discovery with Adoptium Temurin auto-download fallback, content-addressed jar cache, RCON-based lifecycle, source bundles, snapshot-based version updates.

Tracks the Minecraft 26.x series; default scaffold pins 26.1.2 + Java 26. Tested on Linux with system Java 26. Windows process management and the hardlink-based cache are implemented but not CI-tested.

## Install

From crates.io:

```bash
cargo install mc-snap
```

Or from source:

```bash
git clone <repo> mc-snap
cd mc-snap
cargo build --release
# binary lives at target/release/mc-snap
```

Or use the helper script, which runs `cargo install` into either your user or system prefix:

```bash
./scripts/install.sh            # installs to ~/.cargo/bin
./scripts/install.sh --system   # installs to /usr/local/bin (needs sudo)
./scripts/install.sh --debug    # build in debug mode for faster iteration
```

Requires a system `java` (any version) for the doctor probe. Minecraft 26.x needs Java 26 at runtime; if your system Java is older, mc-snap auto-downloads Temurin 26 into `~/.local/share/mc-snap/jdks/`.

## Quickstart

```bash
mkdir my-server && cd my-server
mc-snap init                   # interactive scaffold
mc-snap install                # downloads server jar + mods, writes lockfile
mc-snap start --detach         # starts the server in the background
mc-snap console list           # send `list` via RCON
mc-snap stop                   # graceful shutdown via RCON

mc-snap pack                   # writes mc-snap-bundle.zip (or pass -o to choose)
# elsewhere:
mc-snap unpack mc-snap-bundle.zip
mc-snap install                # reproduces byte-identical setup
```

## Example mc-snap.yml

```yaml
schema: 1
eula: true

server:
  name: grimwald
  description: the grimwald smp
  minecraft: 26.1.2
  loader:
    type: fabric

runtime:
  java: 26
  memory: 4G
  flags:
    - -XX:+UseG1GC

mods:
  - id: fabric-api
    provider: modrinth
    version: latest
  - url: https://github.com/owner/repo/releases/download/v1.0/mymod.jar
    provider: url
    sha256: abc123...

config:
  server.properties:
    motd: "Welcome to Grimwald"
    max-players: 20
```

## Commands

| Command | Purpose |
|---|---|
| `mc-snap init [--non-interactive]` | Interactive scaffold of a new `mc-snap.yml`; `--non-interactive` writes the default template without prompts |
| `mc-snap validate` | Schema check, no network |
| `mc-snap doctor` | Report discovered Java installs and cache paths |
| `mc-snap install` | Resolve, download, materialize `.mc-snap/server/`, write lockfile |
| `mc-snap start [--detach]` | Start the server (foreground by default) |
| `mc-snap stop` | Graceful stop via RCON |
| `mc-snap restart` | `stop` then `start --detach` |
| `mc-snap status` | Running/stopped + player count (via RCON) |
| `mc-snap logs [-f]` | Tail `logs/latest.log` |
| `mc-snap console [cmd...]` | One-shot RCON command, or interactive shell |
| `mc-snap pack -o out.zip` | Bundle `mc-snap.yml` + `mc-snap.lock` + `configs/` |
| `mc-snap unpack <bundle.zip>` | Extract a bundle into the current directory |
| `mc-snap check --to <ver>` | Per-mod compatibility report against a target Minecraft version; no filesystem changes |
| `mc-snap updatable [--to <ver>]` | With `--to`: yes/no for that version. Without: the newest Minecraft version every mod supports |
| `mc-snap search` | List newer mod versions available for the current Minecraft version |
| `mc-snap update --to <ver> [--skip-missing] [--loader <ver>]` | Snapshot, then update mc-snap.yml + lockfile to a new Minecraft version. Resolves mods against the new version first; prompts if any are missing, or use `--skip-missing` to drop them automatically |
| `mc-snap revert [<id>]` / `--list` | Restore the most recent snapshot (or named one); `--list` shows all snapshots |

## Layout

The tool keeps a clean split between user-owned and tool-owned files:

```
my-server/
├── mc-snap.yml        # you edit; commit this
├── mc-snap.lock       # generated; commit this
├── configs/           # external config files; commit these
└── .mc-snap/          # generated; gitignore this
    ├── server/        # actual Minecraft server root
    ├── snapshots/     # pre-update snapshots of yml + lock + configs (for `revert`)
    ├── state.json     # last-applied lockfile hash
    ├── pid            # present when running (pid + start-token, for reuse detection)
    ├── rcon.secret    # auto-generated RCON password (0600 on Unix)
    └── .lock          # advisory lock held by mutating commands
```

`.gitignore` should include `/.mc-snap/` (the `init` command appends this automatically).

Global cache shared across servers: `~/.local/share/mc-snap/{cache,jdks}/` on Linux, `~/Library/Application Support/mc-snap/` on macOS, `%APPDATA%\mc-snap\mc-snap\data` on Windows.

## RCON

`install` auto-generates a 256-bit RCON password in `.mc-snap/rcon.secret` (mode 0600 on Unix) and writes `enable-rcon=true`, `rcon.password=<secret>`, and `rcon.ip=127.0.0.1` into `server.properties`. `stop`, `status`, and `console` all use this. RCON is plaintext, so we always bind it to loopback — the `rcon.ip` override in your yml is overwritten on every install for that reason.

## Development

```bash
make build       # cargo build (debug — use `cargo build --release` for an optimized binary)
make unit        # cargo test --all-targets (unit + wiremock integration)
make e2e         # full lifecycle against a real Fabric server in .dev-servers/
make all         # unit + e2e
make fmt clippy  # formatting + lints
```

See [DESIGN.md](DESIGN.md) for architecture, trait extension points, and implementation notes.
