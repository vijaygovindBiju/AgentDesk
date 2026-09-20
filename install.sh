#!/usr/bin/env bash
#
# AgentDesk prebuilt Linux installer.
#
# This script intentionally installs a prebuilt release artifact. It never
# invokes Rust, Cargo, Flutter, Android tooling, or a compiler.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/vijaygovindBiju/AgentDesk/master/install.sh | bash
#   curl -fsSL .../install.sh | VERSION=v0.1.0 bash
#
# Optional environment:
#   VERSION=v0.1.0
#   INSTALL_DIR="$HOME/.local/lib/agentdesk"
#   AGENTDESK_REPOSITORY="owner/repository"
#   AGENTDESK_RELEASE_BASE_URL="https://github.com/owner/repository/releases/download"
#   AGENTDESK_BIND="127.0.0.1"
#   AGENTDESK_PORT="8765"
#   AGENTDESK_AGENT="agy"
#   AGENTDESK_ANTIGRAVITY="1"

set -Eeuo pipefail
IFS=$'\n\t'

readonly DEFAULT_REPOSITORY="vijaygovindBiju/AgentDesk"
readonly DEFAULT_INSTALL_DIR="${HOME:-}/.local/lib/agentdesk"
readonly SERVICE_NAME="agentdesk.service"

log() {
  printf 'AgentDesk: %s\n' "$*" >&2
}

fail() {
  printf 'AgentDesk installer error: %s\n' "$*" >&2
  exit 1
}

cleanup() {
  if [[ -n "${WORK_DIR:-}" && -d "$WORK_DIR" ]]; then
    rm -rf -- "$WORK_DIR"
  fi
}
trap cleanup EXIT

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "required command '$1' was not found"
}

validate_setting() {
  local name="$1"
  local value="$2"
  [[ "$value" != *$'\n'* && "$value" != *$'\r'* ]] ||
    fail "$name must not contain newlines"
}

[[ "${OSTYPE:-}" == linux-gnu* ]] || fail "this installer supports Linux only"
[[ "${EUID}" -ne 0 ]] || fail "run as a normal user, not root; the installer uses a systemd user service"

require_command curl
require_command sha256sum
require_command tar
require_command install
require_command systemctl
require_command awk
require_command sed
require_command mktemp

REPOSITORY="${AGENTDESK_REPOSITORY:-$DEFAULT_REPOSITORY}"
RELEASE_BASE_URL="${AGENTDESK_RELEASE_BASE_URL:-https://github.com/${REPOSITORY}/releases/download}"
VERSION="${VERSION:-latest}"
INSTALL_DIR="${INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
BIN_DIR="${HOME}/.local/bin"
CONFIG_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/agentdesk"
SERVICE_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/systemd/user"
ENV_FILE="${CONFIG_DIR}/service.env"
SERVICE_WRAPPER="${INSTALL_DIR}/agentdesk-service"

validate_setting "AGENTDESK_REPOSITORY" "$REPOSITORY"
validate_setting "AGENTDESK_RELEASE_BASE_URL" "$RELEASE_BASE_URL"
validate_setting "VERSION" "$VERSION"
validate_setting "INSTALL_DIR" "$INSTALL_DIR"

[[ "$REPOSITORY" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] ||
  fail "AGENTDESK_REPOSITORY must have the form owner/repository"
[[ "$RELEASE_BASE_URL" =~ ^https://[^[:space:]]+$ ]] ||
  fail "AGENTDESK_RELEASE_BASE_URL must use https://"
[[ "$VERSION" != *"/"* && "$VERSION" != *".."* ]] ||
  fail "VERSION contains an unsafe path component"
[[ "$INSTALL_DIR" = /* ]] || fail "INSTALL_DIR must be an absolute path"
[[ "$INSTALL_DIR" != *[[:space:]]* ]] ||
  fail "INSTALL_DIR must not contain whitespace"

case "$(uname -m)" in
  x86_64|amd64)
    ARCH="x86_64"
    ;;
  aarch64|arm64)
    ARCH="aarch64"
    ;;
  *)
    fail "unsupported CPU architecture '$(uname -m)'; supported architectures are x86_64 and aarch64"
    ;;
esac

if [[ "$VERSION" == "latest" ]]; then
  require_command grep
  API_URL="https://api.github.com/repos/${REPOSITORY}/releases/latest"
  log "resolving the latest release from ${REPOSITORY}"
  RELEASE_JSON="$(curl --fail --silent --show-error --location \
    --proto '=https' --proto-redir '=https' \
    --header 'Accept: application/vnd.github+json' "$API_URL")" ||
    fail "could not query the latest GitHub release"
  VERSION="$(printf '%s\n' "$RELEASE_JSON" |
    sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  [[ -n "$VERSION" ]] || fail "GitHub returned no release tag; publish a release before installing"
fi

[[ "$VERSION" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]] ||
  fail "VERSION must be a release tag such as v0.1.0"

ASSET="AgentDesk-${VERSION}-linux-${ARCH}.tar.gz"
CHECKSUMS_ASSET="SHA256SUMS"
RELEASE_URL="${RELEASE_BASE_URL}/${VERSION}"
WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/agentdesk-install.XXXXXX")"
ARCHIVE_PATH="${WORK_DIR}/${ASSET}"
CHECKSUMS_PATH="${WORK_DIR}/${CHECKSUMS_ASSET}"

download() {
  local url="$1"
  local destination="$2"
  log "downloading $(basename "$destination")"
  curl --fail --silent --show-error --location \
    --proto '=https' --proto-redir '=https' \
    --output "$destination" "$url" ||
    fail "download failed: ${url}"
}

download "${RELEASE_URL}/${CHECKSUMS_ASSET}" "$CHECKSUMS_PATH"
download "${RELEASE_URL}/${ASSET}" "$ARCHIVE_PATH"

EXPECTED_SHA="$(
  awk -v asset="$ASSET" '
    {
      filename = $2
      sub(/^\*/, "", filename)
      if (filename == asset) {
        print $1
        exit
      }
    }
  ' "$CHECKSUMS_PATH"
)"
[[ "$EXPECTED_SHA" =~ ^[A-Fa-f0-9]{64}$ ]] ||
  fail "SHA256SUMS does not contain a valid checksum for ${ASSET}"

ACTUAL_SHA="$(sha256sum "$ARCHIVE_PATH" | awk '{print $1}')"
[[ "$ACTUAL_SHA" == "$EXPECTED_SHA" ]] ||
  fail "checksum verification failed for ${ASSET}"
log "verified SHA-256 checksum for ${ASSET}"

ARCHIVE_ENTRIES="$(tar --list --gzip --file "$ARCHIVE_PATH")"
[[ "$ARCHIVE_ENTRIES" == "agentdesk" ]] ||
  fail "release archive must contain exactly one root-level file named agentdesk"

EXTRACT_DIR="${WORK_DIR}/extract"
mkdir -p "$EXTRACT_DIR"
tar --extract --gzip --file "$ARCHIVE_PATH" --directory "$EXTRACT_DIR" \
  --no-same-owner --no-same-permissions
[[ -f "${EXTRACT_DIR}/agentdesk" ]] ||
  fail "release archive does not contain the expected agentdesk executable"
[[ -x "${EXTRACT_DIR}/agentdesk" ]] ||
  fail "agentdesk in the release archive is not executable"

mkdir -p "$INSTALL_DIR" "$BIN_DIR" "$CONFIG_DIR" "$SERVICE_DIR"
install -m 0755 "${EXTRACT_DIR}/agentdesk" "${INSTALL_DIR}/agentdesk"
ln -sfn "${INSTALL_DIR}/agentdesk" "${BIN_DIR}/agentdesk"

cat > "$SERVICE_WRAPPER" <<EOF
#!/usr/bin/env bash
set -Eeuo pipefail
args=(run --mode agentdesk --bind "\${AGENTDESK_BIND:-127.0.0.1}" --port "\${AGENTDESK_PORT:-8765}")
if [[ "\${AGENTDESK_ANTIGRAVITY:-0}" == "1" ]]; then
  args+=(--antigravity)
fi
if [[ -n "\${AGENTDESK_AGENT:-}" ]]; then
  args+=(--agent "\${AGENTDESK_AGENT}")
fi
exec "${INSTALL_DIR}/agentdesk" "\${args[@]}"
EOF
chmod 0755 "$SERVICE_WRAPPER"

if [[ ! -f "$ENV_FILE" ]]; then
  cat > "$ENV_FILE" <<'EOF'
# AgentDesk service configuration. This file contains no authentication token.
AGENTDESK_BIND=127.0.0.1
AGENTDESK_PORT=8765
# Set AGENTDESK_AGENT to a supported command, for example: agy
# Set AGENTDESK_ANTIGRAVITY=1 when using the Antigravity PTY adapter.
EOF
  chmod 0600 "$ENV_FILE"
fi

cat > "${SERVICE_DIR}/${SERVICE_NAME}" <<EOF
[Unit]
Description=AgentDesk attention and control daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
EnvironmentFile=${ENV_FILE}
ExecStart=${SERVICE_WRAPPER}
Restart=on-failure
RestartSec=5s
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=${CONFIG_DIR}

[Install]
WantedBy=default.target
EOF

systemctl --user daemon-reload
systemctl --user enable --now "$SERVICE_NAME"
sleep 1
systemctl --user is-active --quiet "$SERVICE_NAME" ||
  fail "the AgentDesk service did not start; inspect logs with: systemctl --user status ${SERVICE_NAME} and journalctl --user -u ${SERVICE_NAME}"

log "installed ${VERSION} for Linux ${ARCH}"
log "binary: ${BIN_DIR}/agentdesk"
log "configuration: ${CONFIG_DIR}"
log "service: systemctl --user status ${SERVICE_NAME}"
log "logs: journalctl --user -u ${SERVICE_NAME} -f"
log "Android APK releases are published separately; do not install Flutter, Android SDK, or ADB."
log "for LAN phone access, edit ${ENV_FILE}, set AGENTDESK_BIND, then run: systemctl --user restart ${SERVICE_NAME}"
