#!/usr/bin/env bash
# desc: this dispatcher
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
SELF="$(basename "$0")"

# every sibling *.sh carrying a '# desc:' line is a subcommand
commands() {
    local f
    for f in "${SCRIPT_DIR}"/*.sh; do
        [[ -f "$f" ]] || continue
        [[ "$(readlink -f "$f")" == "$(readlink -f "${BASH_SOURCE[0]}")" ]] && continue
        grep -q '^# desc: ' "$f" && basename "$f" .sh
    done
}

usage() {
    printf 'Usage: %s <command> [options] [env-file...]\n\n' "$SELF"
    printf 'Commands:\n'
    local c
    for c in $(commands); do
        printf '  %-18s %s\n' "$c" "$(sed -n 's/^# desc: //p' "${SCRIPT_DIR}/${c}.sh" | head -1)"
    done
    printf '\nSelf:\n'
    printf '  %-18s %s\n' "uninstall" "Remove the installed scripts and their bin link"
    printf '  %-18s %s\n' "--version" "Show the installed ref"
    printf '\nEvery command accepts -h/--help, -e VAR=val and --env-file PATH.\n'
    printf 'Run "%s <command> --help" for its variables.\n\n' "$SELF"
    printf 'Commands act on the bench in the CURRENT directory, not on %s.\n' "$SCRIPT_DIR"
    exit "${1:-0}"
}

[[ $# -eq 0 ]] && usage 0

CMD="$1"
shift

case "$CMD" in
    -h|--help|help) usage 0 ;;
    uninstall)
        # a checkout is not an install: never let this delete someone's working tree
        if git -C "$SCRIPT_DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
            printf '%s is a git checkout, not an install — delete it with git/rm instead\n' \
                "$SCRIPT_DIR" >&2
            exit 1
        fi
        [[ -x "${SCRIPT_DIR}/install.sh" ]] \
            || { printf 'no install.sh next to %s — remove it by hand\n' "$SCRIPT_DIR" >&2; exit 1; }
        # SCRIPT_DIR is where we actually live, whatever INSTALL_DIR was at install time
        INSTALL_DIR="$SCRIPT_DIR" exec "${SCRIPT_DIR}/install.sh" --uninstall ;;
    --version)
        if [[ -f "${SCRIPT_DIR}/VERSION" ]]; then
            cat "${SCRIPT_DIR}/VERSION"
        else
            git -C "$SCRIPT_DIR" rev-parse --short HEAD 2>/dev/null || echo unknown
        fi
        exit 0 ;;
esac

TARGET="${SCRIPT_DIR}/${CMD}.sh"

if [[ ! -x "$TARGET" ]] || ! grep -q '^# desc: ' "$TARGET" 2>/dev/null; then
    printf 'unknown command: %s\n\n' "$CMD" >&2
    usage 2
fi

FE_CMD="${SELF%.sh} ${CMD}" exec "$TARGET" "$@"
