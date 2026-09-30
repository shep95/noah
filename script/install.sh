#!/usr/bin/env sh
# Installs noah on Linux for the current user from the release on the noah
# website: downloads the archive, checks it against the published SHA-256,
# and runs the install script inside it (~/.local/noah.app, a `noah` command
# in ~/.local/bin, and a launcher entry).
#
#   curl -fsSL https://noah.asherin.com/install.sh | sh
#
# The .deb on the download page is the other way in, for Debian and Ubuntu.
set -eu

base_url="${NOAH_DOWNLOAD_URL:-https://noah.asherin.com/downloads}"

case "$(uname -s)" in
    Linux) ;;
    *) echo "this script installs noah on Linux; downloads for other systems are at https://noah.asherin.com/download" >&2; exit 1 ;;
esac
case "$(uname -m)" in
    x86_64) archive="noah-linux-x86_64.tar.xz" ;;
    *) echo "no noah build for $(uname -m) yet; see https://noah.asherin.com/download" >&2; exit 1 ;;
esac

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -qO "$2" "$1"; }
else
    echo "curl or wget is needed to download noah" >&2
    exit 1
fi
command -v sha256sum >/dev/null 2>&1 || { echo "sha256sum is needed to check the download" >&2; exit 1; }

temp="$(mktemp -d)"
trap 'rm -rf "$temp"' EXIT

echo "downloading $archive…"
fetch "$base_url/$archive" "$temp/$archive"
fetch "$base_url/$archive.sha256" "$temp/$archive.sha256"
expected="$(cut -d' ' -f1 < "$temp/$archive.sha256")"
actual="$(sha256sum "$temp/$archive" | cut -d' ' -f1)"
if [ "$expected" != "$actual" ]; then
    echo "the download does not match its published checksum; not installing" >&2
    exit 1
fi

tar -C "$temp" -xJf "$temp/$archive"
sh "$temp/noah.app/install.sh"
