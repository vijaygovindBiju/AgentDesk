#!/usr/bin/env bash

set -Eeuo pipefail
IFS=$'\n\t'

readonly INSTALL_DIR="${INSTALL_DIR:-${HOME}/.local/lib/agentdesk}"
readonly BIN_LINK="${HOME}/.local/bin/agentdesk"
readonly CONFIG_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/agentdesk"
readonly SERVICE_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/systemd/user"
readonly SERVICE_NAME="agentdesk.service"

[[ "${EUID}" -ne 0 ]] || {
  printf 'AgentDesk uninstall error: run as the installed normal user, not root.\n' >&2
  exit 1
}

if command -v systemctl >/dev/null 2>&1; then
  systemctl --user disable --now "$SERVICE_NAME" >/dev/null 2>&1 || true
  systemctl --user daemon-reload >/dev/null 2>&1 || true
fi

rm -f -- "${SERVICE_DIR}/${SERVICE_NAME}" "$BIN_LINK"
rm -rf -- "$INSTALL_DIR"

if [[ "${PURGE_CONFIG:-0}" == "1" ]]; then
  rm -rf -- "$CONFIG_DIR"
  printf 'AgentDesk removed, including configuration and credentials.\n'
else
  printf 'AgentDesk binaries and service removed.\n'
  printf 'Configuration and credentials remain in %s.\n' "$CONFIG_DIR"
  printf 'To remove them too, rerun with PURGE_CONFIG=1.\n'
fi
