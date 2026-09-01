#!/usr/bin/env bash
# desc: Run restore then sanitize in one go

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

USAGE="Usage: ${FE_CMD:-$(basename "$0")} [options] [env-file...]

Run restore.sh then sanitize.sh with the same options.

Options:
  -h, --help              Show this help
  -e VAR=val, --env       Set/override a variable (repeatable)
      --env-file PATH     Load variables from an env file (repeatable)"

ENV_HELP="No env file is required: anything missing is asked for on the terminal.
Options and env files are forwarded verbatim to both scripts.
See './restore.sh --help' and './sanitize.sh --help' for the variables."

# validate/expose here, then hand the same args to each step
parse_args "$@"

# ask for the site once here; both children inherit it and skip their own prompt
require_bench_dir "${RESTORE_SITES_DIR:-sites}"
pick_site RESTORE_SITE "${RESTORE_SITES_DIR:-sites}"
export RESTORE_SITE

echo "========================================"
echo " RESTORE"
echo "========================================"

"${SCRIPT_DIR}/restore.sh" "$@"

echo
echo "========================================"
echo " SANITIZE"
echo "========================================"

"${SCRIPT_DIR}/sanitize.sh" "$@"

echo
echo "========================================"
echo " RESTORE + SANITIZE COMPLETE"
echo "========================================"
