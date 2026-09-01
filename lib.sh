#!/usr/bin/env bash
# Shared CLI handling. Scripts set USAGE (and optionally ENV_HELP), then:
#   source "$(dirname "$0")/lib.sh"
#   parse_args "$@"
#
# Accepted everywhere:
#   -h | --help
#   -e VAR=val | -eVAR=val | --env VAR=val   (repeatable, wins over env files)
#   --env-file PATH | --env-file=PATH        (repeatable, later file wins)
#   PATH                                     (bare positional = env file)

ENV_FILES=()
_OVERRIDES=()

die() { printf '%s\n' "$*" >&2; exit 1; }

usage() {
    printf '%s\n' "${USAGE}"
    [[ -n "${ENV_HELP:-}" ]] && printf '\n%s\n' "${ENV_HELP}"
    exit "${1:-0}"
}

_add_override() {
    [[ "$1" =~ ^[A-Za-z_][A-Za-z0-9_]*= ]] || die "invalid override: $1 (expected VAR=val)"
    _OVERRIDES+=("$1")
}

load_env() {
    local f kv
    for f in ${ENV_FILES[@]+"${ENV_FILES[@]}"}; do
        [[ -f "$f" ]] || die "env file not found: $f"
        set -a
        # shellcheck disable=SC1090
        . "$f"
        set +a
    done
    for kv in ${_OVERRIDES[@]+"${_OVERRIDES[@]}"}; do
        export "${kv}"
    done
}

# The site-facing scripts act on the bench in the CURRENT directory, not on
# wherever the script itself is installed. Fail loudly when cwd is not a bench.
require_bench_dir() {
    local sites="${1:-sites}"
    [[ -d "$sites" ]] || die "not a bench directory: ${PWD} (no '${sites}/'). cd into your bench first."
}

# Interactive fallbacks, so every command works with no env file at all.
# All three no-op when the variable is already set, so -e / --env-file win.

ask_var() {   # ask_var VAR "Prompt" [default]
    local var="$1" prompt="$2" default="${3:-}" answer=""
    [[ -n "${!var:-}" ]] && return 0
    if [[ ! -t 0 ]]; then
        [[ -n "$default" ]] || die "${var} is required (no terminal to ask on; pass -e ${var}=... or --env-file)"
        printf -v "$var" '%s' "$default"
        return 0
    fi
    read -rp "${prompt}${default:+ [${default}]}: " answer || true
    printf -v "$var" '%s' "${answer:-$default}"
    [[ -n "${!var}" ]] || die "${var} is required"
}

ask_secret() {   # ask_secret VAR "Prompt" — never echoed, never defaulted
    local var="$1" prompt="$2" answer=""
    [[ -n "${!var:-}" ]] && return 0
    [[ -t 0 ]] || die "${var} is required (no terminal to ask on; pass -e ${var}=... or --env-file)"
    read -rsp "${prompt}: " answer || true
    echo
    [[ -n "$answer" ]] || die "${var} is required"
    printf -v "$var" '%s' "$answer"
}

pick_site() {   # pick_site VAR [sites_dir] — auto-picks when the bench has one site
    local var="$1" dir="${2:-sites}" sites=() s
    [[ -n "${!var:-}" ]] && return 0
    mapfile -t sites < <(for f in "${dir}"/*/site_config.json; do [[ -f "$f" ]] && basename "$(dirname "$f")"; done)
    [[ ${#sites[@]} -eq 0 ]] && die "no site found in ${dir}/"
    if [[ ${#sites[@]} -eq 1 ]]; then
        printf -v "$var" '%s' "${sites[0]}"
        echo "Site: ${!var}"
        return 0
    fi
    [[ -t 0 ]] || die "${var} is required (${#sites[@]} sites here, no terminal to ask on)"
    PS3="Pick site: "
    select s in "${sites[@]}"; do
        [[ -n "$s" ]] && { printf -v "$var" '%s' "$s"; return 0; }
    done
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)     usage 0 ;;
            -e|--env)      [[ $# -ge 2 ]] || die "$1 requires VAR=val"; _add_override "$2"; shift 2 ;;
            -e*)           _add_override "${1#-e}"; shift ;;
            --env=*)       _add_override "${1#*=}"; shift ;;
            --env-file)    [[ $# -ge 2 ]] || die "--env-file requires a path"; ENV_FILES+=("$2"); shift 2 ;;
            --env-file=*)  ENV_FILES+=("${1#*=}"); shift ;;
            --)            shift; while [[ $# -gt 0 ]]; do ENV_FILES+=("$1"); shift; done ;;
            -*)            printf 'unknown option: %s\n\n' "$1" >&2; usage 2 ;;
            *)             ENV_FILES+=("$1"); shift ;;
        esac
    done
    load_env
}
