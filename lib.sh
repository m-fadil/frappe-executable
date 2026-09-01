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
