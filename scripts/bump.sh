#!/usr/bin/env bash
# bump.sh - cut a release: bump Cargo.toml, commit, tag, push.
#
# Usage:
#   scripts/bump.sh <version>           # e.g. 0.5.0 or v0.5.0
#   scripts/bump.sh <version> --dry-run # print actions but don't run them
#
# Behavior:
#   1. Validate the version arg (semver-ish: MAJOR.MINOR.PATCH[-pre][+build]).
#   2. Refuse if the tag already exists locally or on the remote.
#   3. Update version in Cargo.toml, refresh Cargo.lock via `cargo check`.
#   4. Stage every modified/untracked file in the worktree.
#   5. Commit as "bump: update version to <version>" (matches existing history).
#   6. Tag as v<version>.
#   7. Push the current branch and the new tag.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

red()   { printf '\033[31m%s\033[0m\n' "$*" >&2; }
green() { printf '\033[32m%s\033[0m\n' "$*"; }
step()  { printf '\n\033[1;36m== %s ==\033[0m\n' "$*"; }
run()   {
    if [ "${DRY_RUN:-0}" = "1" ]; then
        printf '\033[2m+ %s\033[0m\n' "$*"
    else
        printf '\033[2m+ %s\033[0m\n' "$*"
        eval "$@"
    fi
}

usage() {
    sed -n '2,20p' "$0" >&2
    exit 2
}

DRY_RUN=0
VERSION=""
for arg in "$@"; do
    case "$arg" in
        -h|--help) usage ;;
        --dry-run) DRY_RUN=1 ;;
        -*) red "unknown flag: $arg"; usage ;;
        *)
            if [ -n "$VERSION" ]; then
                red "multiple versions given: '$VERSION' and '$arg'"
                usage
            fi
            VERSION="$arg"
            ;;
    esac
done

if [ -z "$VERSION" ]; then
    red "missing version"
    usage
fi

VERSION="${VERSION#v}"
TAG="v$VERSION"

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
    red "version '$VERSION' is not semver (MAJOR.MINOR.PATCH[-pre][+build])"
    exit 2
fi

CURRENT=$(grep -E '^version\s*=\s*"' Cargo.toml | head -n 1 | sed -E 's/version\s*=\s*"([^"]+)".*/\1/')
if [ -z "$CURRENT" ]; then
    red "could not read current version from Cargo.toml"
    exit 1
fi

step "release $CURRENT -> $VERSION"
if [ "$DRY_RUN" = "1" ]; then
    green "(dry-run: no changes will be made)"
fi

BRANCH=$(git rev-parse --abbrev-ref HEAD)
if [ "$BRANCH" = "HEAD" ]; then
    red "detached HEAD; check out a branch before bumping"
    exit 1
fi
echo "branch: $BRANCH"

if git rev-parse --verify --quiet "refs/tags/$TAG" >/dev/null; then
    red "tag $TAG already exists locally"
    exit 1
fi
if git ls-remote --tags origin 2>/dev/null | grep -qE "refs/tags/$TAG(\^\{\})?$"; then
    red "tag $TAG already exists on origin"
    exit 1
fi

if [ "$CURRENT" = "$VERSION" ]; then
    red "Cargo.toml already at $VERSION; nothing to bump"
    exit 1
fi

step "update Cargo.toml"
if [ "$DRY_RUN" = "1" ]; then
    printf '\033[2m+ sed -i "s/version = \\"%s\\"/version = \\"%s\\"/" Cargo.toml\033[0m\n' "$CURRENT" "$VERSION"
else
    # Only replace the first occurrence (the [package] version), not any
    # dependency that happens to match the same literal.
    awk -v cur="$CURRENT" -v new="$VERSION" '
        !replaced && /^version[[:space:]]*=[[:space:]]*"/ {
            sub("\"" cur "\"", "\"" new "\"")
            replaced = 1
        }
        { print }
    ' Cargo.toml > Cargo.toml.tmp
    mv Cargo.toml.tmp Cargo.toml
    new_in_file=$(grep -E '^version\s*=\s*"' Cargo.toml | head -n 1 | sed -E 's/version\s*=\s*"([^"]+)".*/\1/')
    if [ "$new_in_file" != "$VERSION" ]; then
        red "Cargo.toml update failed (got '$new_in_file')"
        exit 1
    fi
fi

step "refresh Cargo.lock"
run "cargo check --quiet --workspace --all-targets"

step "stage all changes"
run "git add -A"

if [ "$DRY_RUN" != "1" ]; then
    if git diff --cached --quiet; then
        red "nothing staged; aborting"
        exit 1
    fi
    echo
    echo "--- staged changes ---"
    git diff --cached --stat
    echo
fi

step "commit"
run "git commit -m 'bump: update version to $VERSION'"

step "tag $TAG"
run "git tag -a '$TAG' -m 'release $VERSION'"

step "push branch + tag"
run "git push origin '$BRANCH'"
run "git push origin '$TAG'"

if [ "$DRY_RUN" = "1" ]; then
    green "dry-run complete; no changes made"
else
    green "released $TAG"
fi
