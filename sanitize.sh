#!/usr/bin/env bash
# desc: Disable outbound integrations on a restored site

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

USAGE="Usage: ${FE_CMD:-$(basename "$0")} [options] [env-file...]

Disable outbound integrations on a restored site so it cannot mail or call out.

Options:
  -h, --help              Show this help
  -e VAR=val, --env       Set/override a variable (repeatable)
      --env-file PATH     Load variables from an env file (repeatable)"

ENV_HELP="Variables:
  SANITIZE_SITE           asked; falls back to RESTORE_SITE, auto-picked when
                          the bench has one site
  SANITIZE_SINGLES        comma-separated single doctypes to disable
                          default: FCM Settings,TP Twilio Settings,TP Exotel Settings
                          set to empty to skip
  SANITIZE_SITES_DIR      default: sites
  SANITIZE_MUTE_EMAILS    default: 1  site_config mute_emails value"

parse_args "$@"

SANITIZE_SITE="${SANITIZE_SITE:-${RESTORE_SITE:-}}"
SANITIZE_SINGLES="${SANITIZE_SINGLES-FCM Settings,TP Twilio Settings,TP Exotel Settings}"
SITES_DIR="${SANITIZE_SITES_DIR:-sites}"
require_bench_dir "${SITES_DIR}"
MUTE_EMAILS="${SANITIZE_MUTE_EMAILS:-1}"

pick_site SANITIZE_SITE "${SITES_DIR}"

echo "[sanitize] Checking site: ${SANITIZE_SITE}"

if [[ ! -f "${SITES_DIR}/${SANITIZE_SITE}/site_config.json" ]]; then
    echo "[sanitize] Site ${SANITIZE_SITE} does not exist under ${SITES_DIR}/"
    exit 1
fi

echo "[sanitize] Muting email queue"

bench --site "${SANITIZE_SITE}" \
    set-config mute_emails "${MUTE_EMAILS}"

ALL_TABLES="$(bench --site "${SANITIZE_SITE}" mariadb -N -B -e 'SHOW TABLES')"

has_table() {
    grep -qxF "$1" <<<"${ALL_TABLES}"
}

SQL="SET SQL_SAFE_UPDATES = 0;"

add_sql() {
    if has_table "$1"; then
        SQL+="
$2"
    else
        echo "[sanitize] skip (table missing): $1"
    fi
}

add_select() {
    if has_table "$1"; then
        [[ -n "${VERIFY}" ]] && VERIFY+="
    UNION ALL
"
        VERIFY+="    SELECT '$2' AS check_name, COUNT(*) AS still_enabled FROM \`$1\` WHERE $3"
    fi
}

echo "[sanitize] Disabling outbound integrations"

add_sql 'tabEmail Account' 'UPDATE `tabEmail Account` SET enable_incoming = 0, enable_outgoing = 0, default_incoming = 0, default_outgoing = 0;'
add_sql 'tabWebhook' 'UPDATE `tabWebhook` SET enabled = 0;'
add_sql 'tabNotification' 'UPDATE `tabNotification` SET enabled = 0;'
add_sql 'tabAuto Repeat' 'UPDATE `tabAuto Repeat` SET status = "Disabled" WHERE status = "Active";'
add_sql 'tabEmail Queue Recipient' 'DELETE FROM `tabEmail Queue Recipient`;'
add_sql 'tabEmail Queue' 'DELETE FROM `tabEmail Queue`;'

bench --site "${SANITIZE_SITE}" mariadb -e "${SQL}"

if [[ -z "${SANITIZE_SINGLES}" ]]; then
    echo "[sanitize] skip (SANITIZE_SINGLES empty)"
elif ! has_table 'tabSingles'; then
    echo "[sanitize] skip (table missing): tabSingles"
else
    # normalise "a , b" -> "a,b" for FIND_IN_SET, and escape quotes
    SINGLES="$(printf '%s' "${SANITIZE_SINGLES}" | sed "s/ *, */,/g; s/'/''/g")"

    echo "[sanitize] Disabling single doctypes: ${SINGLES}"

    bench --site "${SANITIZE_SITE}" mariadb -e "
        SET SQL_SAFE_UPDATES = 0;

        UPDATE tabSingles
        SET value = '0'
        WHERE field = 'enabled'
          AND FIND_IN_SET(doctype, '${SINGLES}');

        SELECT
            'singles still enabled' AS check_name,
            COUNT(*) AS still_enabled
        FROM tabSingles
        WHERE field = 'enabled'
          AND value = '1';
    "
fi

echo "[sanitize] Clearing cache"

bench --site "${SANITIZE_SITE}" clear-cache

echo "[sanitize] Verifying remaining enabled integrations"

VERIFY=""
add_select 'tabEmail Account' 'email accounts' 'enable_incoming = 1 OR enable_outgoing = 1'
add_select 'tabWebhook' 'webhooks' 'enabled = 1'
add_select 'tabNotification' 'notifications' 'enabled = 1'
add_select 'tabAuto Repeat' 'auto repeats' 'status = "Active"'
add_select 'tabEmail Queue' 'queued emails' '1 = 1'

if [[ -n "${VERIFY}" ]]; then
    bench --site "${SANITIZE_SITE}" mariadb -e "${VERIFY};"
fi

echo "[sanitize] Done"
