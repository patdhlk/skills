#!/usr/bin/env bash
#
# install-ubc.sh — download and install the `ubc` CLI (per-platform binary).
#
# The binary is public (no auth) and hosted at:
#   https://download.useblocks.com/ubc/<version>/ubc-linux-<arch>-<version>
#
# This script is safe to run inside the devcontainer build, but is also
# reusable on a bare Linux/macOS host. The version can be passed as the first
# argument, or via the UBC_VERSION env var, and defaults to a known-good pin.
#
# Usage:
#   ./install-ubc.sh [version]
#   UBC_VERSION=0.29.3 ./install-ubc.sh

set -euo pipefail

VERSION="${1:-${UBC_VERSION:-0.29.3}}"
DEST="/usr/local/bin/ubc"

# Map the host machine architecture to the names useblocks publishes.
MACHINE="$(uname -m)"
case "${MACHINE}" in
    x86_64)
        ARCH="x64"
        ;;
    aarch64 | arm64)
        ARCH="arm64"
        ;;
    *)
        echo "ERROR: unsupported architecture '${MACHINE}' (expected x86_64, aarch64, or arm64)" >&2
        exit 1
        ;;
esac

URL="https://download.useblocks.com/ubc/${VERSION}/ubc-linux-${ARCH}-${VERSION}"

echo "==> Installing ubc ${VERSION} (${ARCH})"
echo "    Source: ${URL}"
echo "    Target: ${DEST}"

curl -fsSL "${URL}" -o "${DEST}"
chmod +x "${DEST}"

echo "==> ubc installed; verifying version:"
ubc --version || true
