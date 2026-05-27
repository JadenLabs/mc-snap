# mc-snap

Declarative Minecraft server management. Write one YAML file pinning the Minecraft version, loader, mods, Java runtime, and configs; `mc-snap` resolves and downloads everything, manages the server process, and produces a shareable bundle so anyone can reproduce the exact same server with one command.

Think "docker-compose for Minecraft servers".

## Status

Working: Vanilla and Fabric loaders, Modrinth and direct-URL mod providers, install / start / stop / status / logs / console / pack / unpack / validate / doctor, system Java discovery with Adoptium Temurin auto-download fallback, content-addressed jar cache, RCON-based lifecycle, source bundles.

Tested on Linux (system Java 21+). Windows hardlink path exists but is untested.

## Install

```bash
git clone <repo> mc-snap
cd mc-snap
cargo build --release
# binary lives at target/release/mc-snap
```

Requires a system `java` (any version) for the doctor probe; the configured server Java is auto-downloaded if missing.

## Quickstart

```bash
mkdir my-server && cd my-server
mc-snap init                   # interactive scaffold
mc-snap install                # downloads server jar + mods, writes lockfile
mc-snap start --detach         # starts the server in the background
mc-snap console list           # send `list` via RCON
mc-snap stop                   # graceful shutdown via RCON

mc-snap pack -o my-server.zip  # share with friends
# elsewhere:
mc-snap unpack my-server.zip
mc-snap install                # reproduces byte-identical setup
```

## Example mc-snap.yml

```yaml
schema: 1
eula: true

server:
  name: grimwald
  description: the grimwald smp
  minecraft: 1.21.4
  loader:
    type: fabric

runtime:
  java: 21
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
| `mc-snap init` | Interactive scaffold of a new `mc-snap.yml` |
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

## Layout

The tool keeps a clean split between user-owned and tool-owned files:

```
my-server/
├── mc-snap.yml        # you edit; commit this
├── mc-snap.lock       # generated; commit this
├── configs/           # external config files; commit these
└── .mc-snap/          # generated; gitignore this
    ├── server/        # actual Minecraft server root
    ├── state.json     # last-applied lockfile hash
    ├── pid            # present when running
    └── rcon.secret    # auto-generated RCON password
```

Global cache shared across servers: `~/.local/share/mc-snap/{cache,jdks}/`.

## Development

```bash
make build       # cargo build
make unit        # cargo test --workspace (unit + wiremock integration)
make e2e         # full lifecycle against a real Fabric server in .dev-servers/
make all         # unit + e2e
make fmt clippy  # formatting + lints
```

See [DESIGN.md](DESIGN.md) for architecture, trait extension points, and implementation notes.
