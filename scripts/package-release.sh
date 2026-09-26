#!/usr/bin/env bash
# Assemble one system's release archive from the built pieces:
#
#   package-release.sh VERSION SYSTEM REVISION SERVER STATIC MIGRATIONS UI_PKG OUTPUT_DIR
#
# The archive, proofofscore-VERSION-SYSTEM.tar.gz, holds one directory of the
# same name laid out like the Nix package, plus the browser modules:
#
#   bin/server
#   share/proofofscore/static/       bundled JS and CSS (build.rs output)
#   share/proofofscore/migrations/   SQLite migrations
#   share/proofofscore/ui/pkg/       nostr_signer and game_engine WASM modules
#   share/proofofscore/REVISION      the source commit
#   LICENSE
#
# A proofofscore-VERSION-SYSTEM.tar.gz.sha256 beside it is in sha256sum format.
# Timestamps, owners, and order are fixed, so the same inputs give the same bytes.
set -euo pipefail

if [ "$#" -ne 8 ]; then
  sed -n '2,4p' "$0" >&2
  exit 2
fi
version=$1 system=$2 revision=$3 server=$4 static=$5 migrations=$6 ui_pkg=$7 output=$8

[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo "bad version: $version" >&2; exit 1; }
case $system in
  x86_64-linux) machine="x86-64" ;;
  aarch64-linux) machine="ARM aarch64" ;;
  *) echo "unsupported system: $system" >&2; exit 1 ;;
esac
[[ $revision =~ ^[0-9a-f]{40}$ ]] || { echo "bad revision: $revision" >&2; exit 1; }
file -b "$server" | grep -q "ELF 64-bit LSB .*, ${machine}," || { echo "$server is not a $system executable" >&2; exit 1; }
for required in "$static/app.min.js" "$static/loader.js" "$migrations" \
  "$ui_pkg/nostr_signer/nostr_signer.js" "$ui_pkg/nostr_signer/nostr_signer_bg.wasm" \
  "$ui_pkg/game_engine/game_engine.js" "$ui_pkg/game_engine/game_engine_bg.wasm"; do
  [ -e "$required" ] || { echo "missing $required" >&2; exit 1; }
done
ls "$migrations"/*.sql >/dev/null

name="proofofscore-$version-$system"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
root="$work/$name"
share="$root/share/proofofscore"
mkdir -p "$root/bin" "$share/ui/pkg"
install -m 0755 "$server" "$root/bin/server"
cp -R "$static" "$share/static"
cp -R "$migrations" "$share/migrations"
for module in nostr_signer game_engine; do
  # wasm-pack's package.json, README, and .gitignore aren't served.
  mkdir "$share/ui/pkg/$module"
  cp "$ui_pkg/$module"/*.js "$ui_pkg/$module"/*.wasm "$share/ui/pkg/$module/"
  cp "$ui_pkg/$module"/*.d.ts "$share/ui/pkg/$module/" 2>/dev/null || true
done
printf '%s\n' "$revision" > "$share/REVISION"
install -m 0644 LICENSE "$root/LICENSE"
find "$root" -type d -exec chmod 0755 {} +
find "$root" -type f ! -path "$root/bin/server" -exec chmod 0644 {} +

mkdir -p "$output"
archive="$output/$name.tar.gz"
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner --format=gnu \
  -C "$work" -cf - "$name" | gzip -9 -n > "$archive"
(cd "$output" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
cat "$archive.sha256"
tar -tzvf "$archive"
