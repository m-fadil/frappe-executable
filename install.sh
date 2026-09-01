#!/usr/bin/env bash
#
# curl -fsSL https://raw.githubusercontent.com/m-fadil/frappe-executable/main/install.sh | bash
#
set -euo pipefail

REPO="${REPO:-m-fadil/frappe-executable}"
REF="${REF:-main}"
INSTALL_DIR="${INSTALL_DIR:-/opt/frappe-executable}"
BIN_DIR="${BIN_DIR:-/usr/local/bin}"
BIN_NAME="${BIN_NAME:-frappe-exec}"

die() { printf '[install] %s\n' "$*" >&2; exit 1; }
log() { printf '[install] %s\n' "$*"; }

usage() {
    cat <<USAGE
Usage: install.sh [--uninstall] [--help]

Installs the scripts to INSTALL_DIR and links BIN_NAME into BIN_DIR.

Environment:
  REPO          default: ${REPO}
  REF           branch, tag or commit; default: ${REF}
  INSTALL_DIR   default: ${INSTALL_DIR}
  BIN_DIR       default: ${BIN_DIR}
  BIN_NAME      default: ${BIN_NAME}

Piped usage:
  curl -fsSL https://raw.githubusercontent.com/${REPO}/${REF}/install.sh | bash
  curl -fsSL .../install.sh | INSTALL_DIR=~/.local/share/frappe-executable BIN_DIR=~/.local/bin bash
  curl -fsSL .../install.sh | bash -s -- --uninstall
USAGE
    exit "${1:-0}"
}

# sudo only when the target really is not writable (keeps ~/.local installs sudo-free)
nearest_existing() {
    local d="$1"
    while [[ ! -e "$d" ]]; do d="$(dirname "$d")"; done
    printf '%s' "$d"
}

SUDO=""
detect_sudo() {
    [[ -w "$(nearest_existing "$INSTALL_DIR")" && -w "$(nearest_existing "$BIN_DIR")" ]] && return 0
    command -v sudo >/dev/null || die "need write access to ${INSTALL_DIR} and ${BIN_DIR}, and sudo is not available"
    SUDO="sudo"
    log "using sudo to write ${INSTALL_DIR} and ${BIN_DIR}"
}

uninstall() {
    $SUDO rm -f "${BIN_DIR}/${BIN_NAME}"
    $SUDO rm -f "${INSTALL_DIR}"/*.sh "${INSTALL_DIR}/README.md" "${INSTALL_DIR}/.env.example"
    $SUDO rmdir "${INSTALL_DIR}" 2>/dev/null || true
    log "removed ${BIN_DIR}/${BIN_NAME} and ${INSTALL_DIR}"
    exit 0
}

case "${1:-}" in
    -h|--help)   usage 0 ;;
    --uninstall) detect_sudo; uninstall ;;
    "")          ;;
    *)           printf '[install] unknown option: %s\n\n' "$1" >&2; usage 2 ;;
esac

detect_sudo

for c in curl tar; do command -v "$c" >/dev/null || die "missing dependency: $c"; done

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

log "downloading ${REPO}@${REF}"
curl -fsSL "https://github.com/${REPO}/archive/${REF}.tar.gz" \
    | tar -xz -C "$TMP" --strip-components=1 \
    || die "download failed (check REPO/REF)"

[[ -f "${TMP}/frappe-exec.sh" ]] || die "archive has no frappe-exec.sh — wrong REPO/REF?"

log "installing into ${INSTALL_DIR}"
$SUDO mkdir -p "$INSTALL_DIR" "$BIN_DIR"
$SUDO rm -f "${INSTALL_DIR}"/*.sh                 # drop scripts removed upstream
$SUDO cp "${TMP}"/*.sh "$INSTALL_DIR/"
[[ -f "${TMP}/README.md" ]]    && $SUDO cp "${TMP}/README.md" "$INSTALL_DIR/"
[[ -f "${TMP}/.env.example" ]] && $SUDO cp "${TMP}/.env.example" "$INSTALL_DIR/"
$SUDO chmod +x "${INSTALL_DIR}"/*.sh

$SUDO ln -sfn "${INSTALL_DIR}/frappe-exec.sh" "${BIN_DIR}/${BIN_NAME}"

"${BIN_DIR}/${BIN_NAME}" --help >/dev/null || die "smoke test failed"

log "done"
log "reference env file: ${INSTALL_DIR}/.env.example"
echo
if ! command -v "$BIN_NAME" >/dev/null; then
    log "NOTE: ${BIN_DIR} is not on your PATH — add it:"
    log "  export PATH=\"${BIN_DIR}:\$PATH\""
    echo
fi
"${BIN_DIR}/${BIN_NAME}" --help
