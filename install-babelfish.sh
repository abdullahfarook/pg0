#!/bin/bash
# Install pg0-babelfish and localdb (local Babelfish / T-SQL instances) from a GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/abdullahfarook/pg0/main/install-babelfish.sh | bash
#   INSTALL_DIR=/usr/local/bin sudo -E bash install-babelfish.sh      # system-wide
#
# Env: REPO, VERSION (tag; default latest), INSTALL_DIR (default ~/.local/bin),
#      PG0_BABELFISH_URL / LOCALDB_URL (override a download, supports file://).
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
REPO="${REPO:-abdullahfarook/pg0}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

die() { echo -e "${RED}$*${NC}" >&2; exit 1; }

[ "$(uname -s)" = "Linux" ] || die "Babelfish builds are Linux-only for now (glibc, x86_64 or aarch64)."
case "$(uname -m)" in
    x86_64|amd64)  arch="x86_64";;
    arm64|aarch64) arch="aarch64";;
    *) die "Unsupported architecture: $(uname -m)";;
esac
# The bundle is built on Ubuntu 22.04: it needs glibc >= 2.35 and has no static (musl) variant.
if [ -e "/lib/ld-musl-${arch}.so.1" ]; then
    die "musl (Alpine) is not supported by the Babelfish build; use a glibc distro."
fi
if command -v ldd >/dev/null 2>&1; then
    glibc=$(ldd --version 2>&1 | head -n1 | grep -oE '[0-9]+\.[0-9]+' | head -n1 || true)
    if [ -n "$glibc" ] && [ "$(printf '%s\n2.35\n' "$glibc" | sort -V | head -n1)" != "2.35" ]; then
        die "glibc ${glibc} is too old; the Babelfish build needs 2.35+ (Ubuntu 22.04 / Debian 12 or newer)."
    fi
fi
platform="linux-${arch}-gnu"

if [ -z "${PG0_BABELFISH_URL:-}" ] || [ -z "${LOCALDB_URL:-}" ]; then
    version="${VERSION:-}"
    if [ -z "$version" ]; then
        auth=(); [ -n "${GITHUB_TOKEN:-}" ] && auth=(-H "Authorization: Bearer ${GITHUB_TOKEN}")
        version=$(curl -fsSL "${auth[@]}" "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
        [ -n "$version" ] || die "Failed to fetch the latest release of ${REPO}"
    fi
    base="https://github.com/${REPO}/releases/download/${version}"
    PG0_BABELFISH_URL="${PG0_BABELFISH_URL:-${base}/pg0-babelfish-${platform}}"
    LOCALDB_URL="${LOCALDB_URL:-${base}/localdb-${platform}}"
    echo "Installing Babelfish tools ${version} (${platform})..."
fi

fetch() { # url dest
    case "$1" in
        file://*) cp "${1#file://}" "$2";;
        *) curl -fsSL "$1" -o "$2" || die "Failed to download $1";;
    esac
}

mkdir -p "$INSTALL_DIR"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
fetch "$PG0_BABELFISH_URL" "$tmp/pg0-babelfish"
fetch "$LOCALDB_URL" "$tmp/localdb"
chmod +x "$tmp/pg0-babelfish" "$tmp/localdb"
# localdb looks for pg0-babelfish next to itself, so the two must be installed together.
mv "$tmp/pg0-babelfish" "$INSTALL_DIR/pg0-babelfish"
mv "$tmp/localdb" "$INSTALL_DIR/localdb"

echo -e "${GREEN}Installed pg0-babelfish and localdb to ${INSTALL_DIR}${NC}"
if [[ ":$PATH:" != *":${INSTALL_DIR}:"* ]]; then
    echo -e "${YELLOW}NOTE: ${INSTALL_DIR} is not in your PATH. Add: export PATH=\"\$PATH:${INSTALL_DIR}\"${NC}"
fi
echo
echo "Try:  localdb create dev -s && localdb info dev"
