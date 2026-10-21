#!/usr/bin/env bash
#
# Updater for FrameAudioAutoRepatch
#
# Usage:
#   ./update.sh
#   ./update.sh --force
#
set -euo pipefail

REPO="OverlordPotato1/FrameAudioAutoRepatch"
BIN_NAME="FrameAudioAutoRepatch"
SERVICE_NAME="frame-audio-auto-repatch"

INSTALL_DIR="${HOME}/.local/bin"
BIN_PATH="${INSTALL_DIR}/${BIN_NAME}"

[ -x "$BIN_PATH" ] || { echo "${BIN_PATH} not found. Run install.sh first." >&2; exit 1; }

# Download
case "$(uname -m)" in
  aarch64|arm64) TARGET="aarch64-unknown-linux-gnu" ;;
  *)             TARGET="x86_64-unknown-linux-gnu" ;;
esac

ARCHIVE_URL="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep -o '"browser_download_url": *"[^"]*'"${TARGET}"'\.tar\.gz"' \
  | head -n1 | sed 's/.*"\([^"]*\)"$/\1/')"
ARCHIVE_NAME="$(basename "$ARCHIVE_URL")"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

curl -fsSL -o "${TMP}/${ARCHIVE_NAME}" "$ARCHIVE_URL"
curl -fsSL -o "${TMP}/${ARCHIVE_NAME}.sha256" "${ARCHIVE_URL}.sha256"
(cd "$TMP" && sha256sum --check --quiet "${ARCHIVE_NAME}.sha256")

tar -xzf "${TMP}/${ARCHIVE_NAME}" -C "$TMP"


if [ "${1:-}" != "--force" ] && cmp -s "${TMP}/${BIN_NAME}" "$BIN_PATH"; then
  echo "Already up to date."
  exit 0
fi


# Replace binary & restart service
install -m 0755 "${TMP}/${BIN_NAME}" "${BIN_PATH}.new"
mv -f "${BIN_PATH}.new" "$BIN_PATH"
systemctl --user restart "${SERVICE_NAME}.service" 2>/dev/null || true
echo "Updated."