#!/usr/bin/env bash
#
# Installer for FrameAudioAutoRepatch
#
# Usage:
#   ./install.sh
#   ./install.sh --uninstall
#
set -euo pipefail

REPO="OverlordPotato1/FrameAudioAutoRepatch"
BIN_NAME="FrameAudioAutoRepatch"
SERVICE_NAME="frame-audio-auto-repatch"

INSTALL_DIR="${HOME}/.local/bin"
UNIT_FILE="${HOME}/.config/systemd/user/${SERVICE_NAME}.service"

# Uninstall
if [ "${1:-}" = "--uninstall" ]; then
  systemctl --user disable --now "${SERVICE_NAME}.service" 2>/dev/null || true
  rm -f "$UNIT_FILE" "${INSTALL_DIR}/${BIN_NAME}"
  systemctl --user daemon-reload
  exit 0
fi




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





# Install binary & service
mkdir -p "$INSTALL_DIR" "$(dirname "$UNIT_FILE")"
systemctl --user stop "${SERVICE_NAME}.service" 2>/dev/null || true
install -m 0755 "${TMP}/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"

cat > "$UNIT_FILE" <<EOF
[Unit]
Description=FrameAudioAutoRepatch - automatically re-patch PipeWire audio links
After=pipewire.service wireplumber.service pipewire-pulse.service
Wants=pipewire.service

[Service]
ExecStart=${INSTALL_DIR}/${BIN_NAME}
Restart=on-failure
RestartSec=3

[Install]
WantedBy=default.target
EOF

systemctl --user daemon-reload
systemctl --user enable --now "${SERVICE_NAME}.service"