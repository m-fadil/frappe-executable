#!/usr/bin/env bash
# Self-check for lib.sh arg parsing. Run: ./test_lib.sh
set -uo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

printf 'A=from_file\nB=b_file\n' > "$TMP/one.env"
printf 'A=second_file\n' > "$TMP/two.env"

# run parse_args in a subshell and echo the resulting vars
run() {
    bash -c '
        set -euo pipefail
        source "$1/lib.sh"
        USAGE="usage-text"
        parse_args "${@:3}"
        echo "A=${A:-} B=${B:-} C=${C:-} FILES=${#ENV_FILES[@]}"
    ' _ "$SCRIPT_DIR" -- "$@" 2>&1
}

fail=0
check() {
    local desc="$1" want="$2" got="$3"
    if [[ "$got" == *"$want"* ]]; then
        echo "ok   - $desc"
    else
        echo "FAIL - $desc"
        echo "       want substring: $want"
        echo "       got:            $got"
        fail=1
    fi
}

check "bare positional loads env file" \
    "A=from_file B=b_file C= FILES=1" "$(run "$TMP/one.env")"

check "--env-file loads env file" \
    "A=from_file B=b_file C= FILES=1" "$(run --env-file "$TMP/one.env")"

check "--env-file=PATH form" \
    "A=from_file" "$(run --env-file="$TMP/one.env")"

check "later env file wins" \
    "A=second_file" "$(run "$TMP/one.env" "$TMP/two.env")"

check "-e overrides env file regardless of order" \
    "A=cli B=b_file" "$(run -e A=cli "$TMP/one.env")"

check "-eVAR=val attached form" \
    "A=cli" "$(run -eA=cli)"

check "--env VAR=val form" \
    "C=three" "$(run --env C=three)"

check "repeatable -e" \
    "A=1 B=2" "$(run -e A=1 -e B=2)"

check "value with = and spaces survives" \
    "C=a=b c" "$(run -e "C=a=b c")"

check "-h prints usage" "usage-text" "$(run -h)"
check "--help prints usage" "usage-text" "$(run --help)"
check "unknown option rejected" "unknown option: --nope" "$(run --nope)"
check "malformed override rejected" "invalid override: notanassignment" "$(run -e notanassignment)"
check "missing env file rejected" "env file not found" "$(run --env-file "$TMP/absent.env")"
check "-e without value rejected" "requires VAR=val" "$(run -e)"

# every script must answer --help without touching bench
for s in bench-init.sh create-user.sh restore.sh sanitize.sh restore-sanitize.sh; do
    out="$("$SCRIPT_DIR/$s" --help 2>&1)"; rc=$?
    check "$s --help exits 0 and prints usage" "Usage: $s" "$out"
    [[ $rc -eq 0 ]] || { echo "FAIL - $s --help exit $rc"; fail=1; }
done

# .env.example is sourced by bash, so it must parse with nothing set
if [[ -f "$SCRIPT_DIR/.env.example" ]]; then
    out="$(bash -c 'set -euo pipefail; set -a; . "$1"; set +a' _ "$SCRIPT_DIR/.env.example" 2>&1)"
    if [[ -z "$out" ]]; then
        echo "ok   - .env.example sources cleanly"
    else
        echo "FAIL - .env.example does not source cleanly"
        echo "       $out"
        fail=1
    fi

    # and every variable it names must actually be read by some script
    for v in $(grep -oE '^#? *[A-Z][A-Z0-9_]*=' "$SCRIPT_DIR/.env.example" | tr -d '# ' | tr -d '='); do
        grep -qF "$v" "$SCRIPT_DIR"/*.sh || { echo "FAIL - .env.example names unused var: $v"; fail=1; }
    done
    echo "ok   - .env.example names no unused variables"
fi

exit "$fail"
