#!/usr/bin/env bash
#
# curl -fsSL https://raw.githubusercontent.com/m-fadil/frappe-executable/main/install.sh | bash
#
set -euo pipefail

REPO="${REPO:-m-fadil/frappe-executable}"
REF="${REF:-main}"
INSTALL_DIR="${INSTALL_DIR:-/opt/frappe-executable}"
BIN_DIR="${BIN_DIR:-/usr/local/bin}"
BIN_NAME="${BIN_NAME:-frappe}"

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
  frappe uninstall               # same thing, finds its own install dir
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

    command -v sudo >/dev/null || no_sudo "sudo is not installed"

    # Get the credential up front. sudo reads the password from /dev/tty, so this
    # works under 'curl | bash' too, but fails flatly when there is no terminal.
    if ! sudo -n true 2>/dev/null; then
        log "sudo password needed to write ${INSTALL_DIR} and ${BIN_DIR}"
        sudo -v || no_sudo "sudo could not authenticate (no terminal to ask on?)"
    fi

    SUDO="sudo"
    log "using sudo for ${INSTALL_DIR} and ${BIN_DIR}"
}

no_sudo() {
    die "$1

${INSTALL_DIR} and ${BIN_DIR} are not writable. Either:
  run the whole thing as root:
    curl -fsSL https://raw.githubusercontent.com/${REPO}/${REF}/install.sh | sudo bash
  or install rootless, no sudo at all:
    curl -fsSL https://raw.githubusercontent.com/${REPO}/${REF}/install.sh \\
      | INSTALL_DIR=\"\$HOME/.local/share/frappe-executable\" BIN_DIR=\"\$HOME/.local/bin\" bash"
}

# Every symlink on PATH (plus the usual bin dirs) that resolves to our entry,
# so a custom BIN_DIR/BIN_NAME still gets cleaned up.
entry_links() {
    local target="$1" dirs d f
    IFS=: read -ra dirs <<< "${PATH}"
    dirs+=("$BIN_DIR" /usr/local/bin /usr/bin "${HOME}/.local/bin" "${HOME}/bin")
    while read -r d; do
        [[ -d "$d" ]] || continue
        for f in "$d"/*; do
            [[ -L "$f" ]] || continue
            [[ "$(readlink -f "$f" 2>/dev/null)" == "$target" ]] && printf '%s\n' "$f"
        done
    done < <(printf '%s\n' "${dirs[@]}" | awk 'NF' | sort -u)
}

uninstall() {
    local entry="${INSTALL_DIR}/frappe-exec.sh"

    # refuse to delete *.sh out of a directory that is not one of our installs
    [[ -f "$entry" ]] || die "not an install directory: ${INSTALL_DIR} (no frappe-exec.sh). Set INSTALL_DIR."

    # the link install.sh made may live outside PATH, so trust the record first
    local recorded=""
    if [[ -f "${INSTALL_DIR}/.bin-link" ]]; then
        recorded="$(<"${INSTALL_DIR}/.bin-link")"
        [[ -n "$recorded" ]] && BIN_DIR="$(dirname "$recorded")"
    fi

    detect_sudo

    local target link removed=0
    target="$(readlink -f "$entry")"

    while read -r link; do
        [[ -n "$link" ]] || continue
        [[ -L "$link" ]] || continue
        [[ "$(readlink -f "$link" 2>/dev/null)" == "$target" ]] || continue
        $SUDO rm -f "$link"
        log "removed link ${link}"
        removed=$((removed + 1))
    done < <({ [[ -n "$recorded" ]] && printf '%s\n' "$recorded"; entry_links "$target"; } | awk 'NF' | sort -u)
    [[ $removed -eq 0 ]] && log "no bin link found pointing at ${entry}"

    $SUDO rm -f "${INSTALL_DIR}"/*.sh "${INSTALL_DIR}/README.md" \
        "${INSTALL_DIR}/.env.example" "${INSTALL_DIR}/VERSION" "${INSTALL_DIR}/.bin-link"
    log "removed scripts from ${INSTALL_DIR}"

    if $SUDO rmdir "${INSTALL_DIR}" 2>/dev/null; then
        log "removed ${INSTALL_DIR}"
    else
        log "kept ${INSTALL_DIR} — it still holds files we did not install:"
        ls -A "${INSTALL_DIR}" | sed 's/^/[install]   /'
    fi

    log "uninstalled"
    exit 0
}

case "${1:-}" in
    -h|--help)   usage 0 ;;
    --uninstall) uninstall ;;
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

# stamp what we installed; the tarball carries no .git for --version to read
printf '%s\n' "${REF}" | $SUDO tee "${INSTALL_DIR}/VERSION" >/dev/null

$SUDO ln -sfn "${INSTALL_DIR}/frappe-exec.sh" "${BIN_DIR}/${BIN_NAME}"
printf '%s\n' "${BIN_DIR}/${BIN_NAME}" | $SUDO tee "${INSTALL_DIR}/.bin-link" >/dev/null

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
