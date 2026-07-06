# mc-snap

Declarative Minecraft server management. Write one YAML file pinning the Minecraft version, loader, mods, Java runtime, and configs; `mc-snap` resolves and downloads everything, manages the server process, and produces a shareable bundle so anyone can reproduce the exact same server with one command.

Think "docker-compose for Minecraft servers".

## Status

Working: Vanilla and Fabric loaders, Modrinth / CurseForge / direct-URL mod providers, datapacks (Modrinth, CurseForge, VanillaTweaks, direct URL), init / install / get / start / stop / restart / status / logs / console / pack / unpack / validate / doctor / check / updatable / search / update / revert / config detect, system Java discovery with Adoptium Temurin auto-download fallback, content-addressed jar cache, RCON-based lifecycle, source bundles, snapshot-based version updates.

Tracks the Minecraft 26.x series; default scaffold pins 26.1.2 + Java 26.

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

No system Java is required: Minecraft 26.x needs Java 26 at runtime, and if your system Java is older (or missing), mc-snap auto-downloads Temurin 26 into `~/.local/share/mc-snap/jdks/`. Run `mc-snap doctor` to see which Java installs were discovered.

## Quickstart

```bash
mkdir my-server && cd my-server
mc-snap init                   # interactive scaffold
mc-snap install                # downloads server jar + mods, writes lockfile
mc-snap get chunky             # add a mod by Modrinth slug and install it
mc-snap start --detach         # starts the server in the background
mc-snap console list           # send `list` via RCON
mc-snap stop                   # graceful shutdown via RCON

mc-snap pack                   # writes mc-snap-bundle.zip (or pass -o to choose)
# elsewhere:
mc-snap unpack mc-snap-bundle.zip
mc-snap install                # reproduces the locked setup exactly
```

`install` reuses the existing `mc-snap.lock` whenever `mc-snap.yml` is unchanged, so a shared bundle installs the exact pinned versions. Pass `--refresh` to re-resolve `version: latest` entries against the registries.

## Example mc-snap.yml

```yaml
schema: 1
eula: true

server:
  name: grimwald
  description: the grimwald smp
  minecraft: 26.1.2
  # location: server         # optional; materialize server files into ./server/
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
  - id: "238222"            # JEI, by CurseForge project id (or slug)
    provider: curseforge
    version: latest
  - url: https://github.com/owner/repo/releases/download/v1.0/mymod.jar
    provider: url
    sha256: abc123...

datapacks:
  - id: terralith
    provider: modrinth
    version: latest
  - provider: vanillatweaks
    version: "26.1"
    packs:
      survival:
        - graves
        - afk_display
  - url: https://example.com/my-pack.zip
    provider: url
    sha256: def456...

config:
  server.properties:
    motd: "Welcome to Grimwald"
    max-players: 20
```

Datapacks install into the world's `datapacks/` directory (`<level-name>/datapacks/`, default `world/datapacks/`).

## CurseForge

The CurseForge v1 API requires a personal API key. Set `CURSEFORGE_API_KEY` (or `CF_API_KEY`) in your environment before running `install` / `update` / `search`; get a key at <https://console.curseforge.com>. Mods can be referenced by numeric project id or slug, and pinned to a specific file id via `version`.

## Commands

| Command | Purpose |
|---|---|
| `mc-snap init [--non-interactive] [--detect [PATH]] [--force] [--no-mod-resolve]` | Interactive scaffold of a new `mc-snap.yml`. `--detect` inspects an existing server directory (jars, mods, server.properties) and pre-fills the wizard; `--non-interactive` skips the prompts; `--force` overwrites an existing yml |
| `mc-snap validate` | Schema check, no network |
| `mc-snap doctor` | Report discovered Java installs, cache paths, and the current project's requirements |
| `mc-snap install [--copy] [--symlink] [--refresh]` | Resolve, download, materialize the server directory, write lockfile. Reuses the existing lockfile when `mc-snap.yml` is unchanged; `--refresh` forces re-resolution. By default links artifacts from the cache (symlink on Unix, hardlink on Windows); `--copy` copies them in instead, `--symlink` forces symlinks on every platform |
| `mc-snap get <slug...> [--version <v>] [--provider <p>] [--no-install]` | Add mods by slug, picking the newest compatible version, then install |
| `mc-snap start [--detach]` | Start the server (foreground by default) |
| `mc-snap stop` | Graceful stop via RCON |
| `mc-snap restart` | `stop` then `start --detach` |
| `mc-snap status` | Running/stopped + player count (via RCON) |
| `mc-snap logs [-f]` | Tail `logs/latest.log` |
| `mc-snap console [cmd...]` | One-shot RCON command, or interactive shell |
| `mc-snap pack -o out.zip` | Bundle `mc-snap.yml` + `mc-snap.lock` + `configs/` |
| `mc-snap unpack <bundle.zip> [--force]` | Extract a bundle into the current directory; refuses to overwrite existing files unless `--force` |
| `mc-snap check --to <ver>` | Per-mod compatibility report against a target Minecraft version; no filesystem changes |
| `mc-snap updatable [--to <ver>]` | With `--to`: yes/no for that version. Without: the newest Minecraft version every mod supports |
| `mc-snap search` | List newer mod versions available for the current Minecraft version |
| `mc-snap update --to <ver> [--skip-missing] [--yes] [--loader <ver>]` | Snapshot, then update mc-snap.yml + lockfile to a new Minecraft version. Resolves mods against the new version first; prompts if any are missing, `--skip-missing` drops them automatically, `--yes` cancels instead of prompting |
| `mc-snap revert [<id>]` / `--list` | Restore the most recent snapshot (or named one); `--list` shows all snapshots |
| `mc-snap config detect [--all]` | Scan the server's `config/` directory and track new mod config files in `configs/` |

## Layout

The tool keeps a clean split between user-owned and tool-owned files. Server files materialize at the project root by default, or under a subdirectory when `server.location` is set:

```
my-server/
├── mc-snap.yml        # you edit; commit this
├── mc-snap.lock       # generated; commit this
├── configs/           # external config files; commit these
├── server.jar         # server files land here (or in ./<server.location>/):
├── mods/              #   linked/copied from the cache
├── world/             #   world data, logs/, server.properties, eula.txt, ...
└── .mc-snap/          # generated; gitignore this
    ├── snapshots/     # pre-update snapshots of yml + lock + configs (for `revert`)
    ├── state.json     # last-applied lockfile hash
    ├── pid            # present when running (pid + start-token, for reuse detection)
    ├── rcon.secret    # auto-generated RCON password (0600 on Unix)
    └── .lock          # advisory lock held by mutating commands
```

`.gitignore` should include `.mc-snap/` (the `init` command appends this automatically, and suggests entries for the generated server files).

Global cache shared across servers: `~/.local/share/mc-snap/{cache,jdks}/` on Linux, `~/Library/Application Support/mc-snap/` on macOS, `%APPDATA%\mc-snap\mc-snap\data` on Windows.

## RCON

`install` auto-generates a 256-bit RCON password in `.mc-snap/rcon.secret` (mode 0600 on Unix) and writes `enable-rcon=true`, `rcon.password=<secret>`, and `rcon.ip=127.0.0.1` into `server.properties`. `stop`, `status`, and `console` all use this. RCON is plaintext, so we always bind it to loopback - the `rcon.ip` override in your yml is overwritten on every install for that reason.

## Development

```bash
make build       # cargo build (debug - use `cargo build --release` for an optimized binary)
make unit        # cargo test --all-targets (unit + wiremock integration)
make e2e         # full lifecycle against a real Fabric server in .dev-servers/
make all         # unit + e2e
make fmt clippy  # formatting + lints
```

See [DESIGN.md](DESIGN.md) for architecture, trait extension points, and implementation notes.
