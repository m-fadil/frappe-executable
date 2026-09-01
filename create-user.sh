#!/usr/bin/env bash
# desc: Create a user on a site, optionally print API credentials

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

USAGE="Usage: ${FE_CMD:-$(basename "$0")} [options] [env-file...]

Create (or reuse) a user on a site in the current bench.

Options:
  -h, --help              Show this help
  -e VAR=val, --env       Set/override a variable (repeatable)
      --env-file PATH     Load variables from an env file (repeatable)"

ENV_HELP="Variables:
  USER_SITE               site name; single site is auto-picked, otherwise asked
  USER_EMAIL              required; asked when unset
  USER_FIRST_NAME         asked when unset, defaults to the email local part
  USER_LAST_NAME          optional
  USER_ROLES              comma-separated roles, default: System Manager
                          set to empty for no extra roles
  USER_PASSWORD           optional; set only when non-empty
  USER_API_KEY            default: 1  generate/print api_key:api_secret
  USER_WELCOME_EMAIL      default: 0  send the welcome email
  USER_SITES_DIR          default: sites

Examples:
  ${FE_CMD:-$(basename "$0")} -e USER_EMAIL=mcp@example.com
  ${FE_CMD:-$(basename "$0")} -e USER_EMAIL=ops@example.com -e USER_FIRST_NAME=Ops \\
      -e USER_ROLES='Accounts User,Sales User' -e USER_API_KEY=0
  ${FE_CMD:-$(basename "$0")} --env-file user.env"

parse_args "$@"

SITES_DIR="${USER_SITES_DIR:-sites}"
require_bench_dir "${SITES_DIR}"

pick_site USER_SITE "${SITES_DIR}"

[ -f "${SITES_DIR}/${USER_SITE}/site_config.json" ] || die "Site ${USER_SITE} does not exist under ${SITES_DIR}/"

ask_var USER_EMAIL "User email"
[[ "${USER_EMAIL}" == *@*.* ]] || die "not an email address: ${USER_EMAIL}"

ask_var USER_FIRST_NAME "First name" "${USER_EMAIL%%@*}"
[ -n "${USER_FIRST_NAME}" ] || die "USER_FIRST_NAME is required"

export USER_EMAIL USER_FIRST_NAME
export USER_LAST_NAME="${USER_LAST_NAME:-}"
export USER_ROLES="${USER_ROLES-System Manager}"
export USER_PASSWORD="${USER_PASSWORD:-}"
export USER_API_KEY="${USER_API_KEY:-1}"
export USER_WELCOME_EMAIL="${USER_WELCOME_EMAIL:-0}"

echo "Site       : ${USER_SITE}"
echo "Email      : ${USER_EMAIL}"
echo "Name       : ${USER_FIRST_NAME}${USER_LAST_NAME:+ ${USER_LAST_NAME}}"
echo "Roles      : ${USER_ROLES:-<none>}"
echo "API key    : $([[ "${USER_API_KEY}" == "0" ]] && echo no || echo yes)"
echo

PY=$(mktemp -t create_user.XXXXXX.py)
trap 'rm -f "$PY"' EXIT

# values come in via the environment, never interpolated into the source
cat > "$PY" <<'PYTHON'
import os

import frappe
from frappe.core.doctype.user.user import generate_keys
from frappe.utils.password import get_decrypted_password


def flag(name, default="0"):
    return os.environ.get(name, default).strip().lower() in ("1", "true", "yes", "y")


email = os.environ["USER_EMAIL"]
roles = [r.strip() for r in os.environ.get("USER_ROLES", "").split(",") if r.strip()]
password = os.environ.get("USER_PASSWORD", "")
welcome = flag("USER_WELCOME_EMAIL")

for role in roles:
    if not frappe.db.exists("Role", role):
        raise SystemExit(f"Role does not exist on this site: {role}")

if frappe.db.exists("User", email):
    user = frappe.get_doc("User", email)
    print(f"User already exists, left unchanged: {email}")
else:
    user = frappe.get_doc({
        "doctype": "User",
        "email": email,
        "first_name": os.environ["USER_FIRST_NAME"],
        "last_name": os.environ.get("USER_LAST_NAME", ""),
        "enabled": 1,
        "send_welcome_email": 1 if welcome else 0,
        "roles": [{"role": r} for r in roles],
    })
    user.flags.no_welcome_mail = not welcome
    if password:
        user.new_password = password
    user.insert(ignore_permissions=True)
    print(f"User created: {email}")
    if roles:
        print(f"Roles: {', '.join(roles)}")

if flag("USER_API_KEY", "1"):
    if user.api_key:
        api_secret = get_decrypted_password("User", user.name, "api_secret", raise_exception=True)
    else:
        api_secret = generate_keys(user.name)["api_secret"]
        user.reload()
        print("API credentials generated")
    print()
    print(f"{user.api_key}:{api_secret}")

frappe.db.commit()
PYTHON

bench --site "$USER_SITE" execute "exec(compile(open('$PY').read(), '$PY', 'exec'))"
