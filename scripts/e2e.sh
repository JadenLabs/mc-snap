#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEV="$ROOT/.dev-servers"
BIN="$ROOT/target/debug/mc-snap"

TEST_DIR="$DEV/e2e"
BUNDLE="$DEV/bundle.zip"
UNPACK_DIR="$DEV/e2e-unpacked"

SERVER_PORT="${MC_SNAP_E2E_PORT:-25569}"
RCON_PORT="${MC_SNAP_E2E_RCON_PORT:-25579}"
MC_VERSION="${MC_SNAP_E2E_MC:-1.21.4}"
BOOT_TIMEOUT="${MC_SNAP_E2E_BOOT_TIMEOUT:-300}"
STOP_TIMEOUT="${MC_SNAP_E2E_STOP_TIMEOUT:-60}"

green()  { printf '\033[32m%s\033[0m\n' "$*"; }
red()    { printf '\033[31m%s\033[0m\n' "$*"; }
step()   { printf '\n\033[1;36m== %s ==\033[0m\n' "$*"; }

cleanup() {
    local rc=$?
    if [ -d "$TEST_DIR" ] && [ -f "$TEST_DIR/.mc-snap/pid" ]; then
        step "cleanup: stopping any running server"
        (cd "$TEST_DIR" && "$BIN" stop || true)
    fi
    if [ "$rc" -ne 0 ]; then
        red "FAILED (exit $rc)"
        if [ -f "$TEST_DIR/.mc-snap/server/logs/latest.log" ]; then
            echo "--- last 40 lines of server log ---"
            tail -n 40 "$TEST_DIR/.mc-snap/server/logs/latest.log"
        fi
    fi
    exit $rc
}
trap cleanup EXIT INT TERM

require() {
    if ! command -v "$1" >/dev/null 2>&1; then
        red "missing required tool: $1"
        exit 2
    fi
}
require java

step "build"
(cd "$ROOT" && cargo build --quiet)

step "detect java"
JAVA_MAJOR=$(java -version 2>&1 | head -1 | sed -E 's/.*"([0-9]+).*/\1/')
if [ -z "$JAVA_MAJOR" ] || ! [[ "$JAVA_MAJOR" =~ ^[0-9]+$ ]]; then
    red "could not parse java major version"
    exit 2
fi
if [ "$JAVA_MAJOR" -lt 21 ]; then
    JAVA_VER=21
    echo "system java $JAVA_MAJOR too old, will let mc-snap fetch Temurin $JAVA_VER"
else
    JAVA_VER=$JAVA_MAJOR
    echo "using system java $JAVA_VER"
fi

step "reset test directory"
rm -rf "$TEST_DIR" "$UNPACK_DIR" "$BUNDLE"
mkdir -p "$TEST_DIR"

cat > "$TEST_DIR/mc-snap.yml" <<EOF
schema: 1
eula: true

server:
  name: e2e-fabric
  description: mc-snap end-to-end test
  minecraft: $MC_VERSION
  loader:
    type: fabric

runtime:
  java: $JAVA_VER
  memory: 2G
  flags:
    - -XX:+UseG1GC

mods:
  - id: fabric-api
    provider: modrinth
    version: latest

config:
  server.properties:
    motd: "mc-snap e2e"
    max-players: 5
    server-port: $SERVER_PORT
    rcon.port: $RCON_PORT
    online-mode: false
EOF

step "validate"
(cd "$TEST_DIR" && "$BIN" validate)

step "doctor"
(cd "$TEST_DIR" && "$BIN" doctor)

step "install (downloads server jar + fabric-api)"
(cd "$TEST_DIR" && "$BIN" install)

if [ ! -f "$TEST_DIR/mc-snap.lock" ]; then
    red "lockfile not written"
    exit 1
fi
if [ ! -f "$TEST_DIR/.mc-snap/server/fabric-server-launch.jar" ]; then
    red "fabric server launcher not in place"
    exit 1
fi
if ! ls "$TEST_DIR/.mc-snap/server/mods/" | grep -qi fabric-api; then
    red "fabric-api mod not in mods folder"
    exit 1
fi
green "install artifacts look correct"

step "idempotent install"
(cd "$TEST_DIR" && "$BIN" install)
green "second install ok"

step "start --detach"
(cd "$TEST_DIR" && "$BIN" start --detach)

step "wait for server to boot (up to ${BOOT_TIMEOUT}s)"
LOG="$TEST_DIR/.mc-snap/server/logs/latest.log"
deadline=$(( $(date +%s) + BOOT_TIMEOUT ))
booted=0
while [ "$(date +%s)" -lt "$deadline" ]; do
    if [ -f "$LOG" ] && grep -q 'Done (' "$LOG" 2>/dev/null; then
        booted=1
        break
    fi
    sleep 2
done
if [ "$booted" -ne 1 ]; then
    red "server did not boot within ${BOOT_TIMEOUT}s"
    exit 1
fi
green "server booted"

step "status (should be running)"
status_out=$(cd "$TEST_DIR" && "$BIN" status)
echo "$status_out"
echo "$status_out" | grep -q running || { red "status did not report running"; exit 1; }

step "rcon: list players"
(cd "$TEST_DIR" && "$BIN" console list)

step "rcon: say hello"
(cd "$TEST_DIR" && "$BIN" console say "hello from mc-snap e2e")

step "logs (last lines)"
(cd "$TEST_DIR" && "$BIN" logs | tail -n 5)

step "stop (graceful via rcon)"
(cd "$TEST_DIR" && "$BIN" stop)

step "wait for stop (up to ${STOP_TIMEOUT}s)"
deadline=$(( $(date +%s) + STOP_TIMEOUT ))
stopped=0
while [ "$(date +%s)" -lt "$deadline" ]; do
    if ! (cd "$TEST_DIR" && "$BIN" status) | grep -q running; then
        stopped=1
        break
    fi
    sleep 1
done
if [ "$stopped" -ne 1 ]; then
    red "server did not stop"
    exit 1
fi
green "server stopped"

step "pack"
(cd "$TEST_DIR" && "$BIN" pack -o "$BUNDLE")
[ -f "$BUNDLE" ] || { red "bundle not written"; exit 1; }
green "bundle: $(du -h "$BUNDLE" | cut -f1)"

step "unpack into fresh directory"
mkdir -p "$UNPACK_DIR"
(cd "$UNPACK_DIR" && "$BIN" unpack "$BUNDLE")
[ -f "$UNPACK_DIR/mc-snap.yml" ]  || { red "unpacked yml missing"; exit 1; }
[ -f "$UNPACK_DIR/mc-snap.lock" ] || { red "unpacked lock missing"; exit 1; }

step "validate unpacked bundle"
(cd "$UNPACK_DIR" && "$BIN" validate)

green ""
green "e2e suite passed"
