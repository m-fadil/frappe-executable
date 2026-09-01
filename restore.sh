#!/usr/bin/env bash
# desc: Pull the latest backup from a source site and restore it

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

USAGE="Usage: ${FE_CMD:-$(basename "$0")} [options] [env-file...]

Pull the latest database backup from a source site and restore it locally.

Options:
  -h, --help              Show this help
  -e VAR=val, --env       Set/override a variable (repeatable)
      --env-file PATH     Load variables from an env file (repeatable)"

ENV_HELP="Variables:
  RESTORE_SITE            required  target site on this bench
  RESTORE_SOURCE_URL      required  source site base URL
  RESTORE_SOURCE_TOKEN    required  Authorization header value
  DB_ROOT_USER            required  database root user
  DB_ROOT_PASS            required  database root password (alias: DB_PASSWORD)
  RESTORE_ADMIN_PASSWORD  required  admin password to set after restore
  RESTORE_DOWNLOAD_DIR    default: \$PWD
  RESTORE_SITES_DIR       default: sites
  RESTORE_CONNECTIONS     default: 4        aria2c -x/-s
  RESTORE_KEEP_BACKUP     default: 1        set to 0 to delete after restore
  RESTORE_SKIP_MIGRATE    default: empty    non-empty skips 'bench migrate'"

parse_args "$@"

: "${RESTORE_SITE:?RESTORE_SITE is required}"
: "${RESTORE_SOURCE_URL:?RESTORE_SOURCE_URL is required}"
: "${RESTORE_SOURCE_TOKEN:?RESTORE_SOURCE_TOKEN is required}"
# DB_ROOT_PASS is the shared name (bench-init.sh uses it too); DB_PASSWORD is
# kept as an alias so older env files keep working.
DB_ROOT_PASS="${DB_ROOT_PASS:-${DB_PASSWORD:-}}"
: "${DB_ROOT_USER:?DB_ROOT_USER is required}"
: "${DB_ROOT_PASS:?DB_ROOT_PASS (or DB_PASSWORD) is required}"
: "${RESTORE_ADMIN_PASSWORD:?RESTORE_ADMIN_PASSWORD is required}"

SOURCE_URL="${RESTORE_SOURCE_URL%/}"
DOWNLOAD_DIR="${RESTORE_DOWNLOAD_DIR:-$PWD}"
SITES_DIR="${RESTORE_SITES_DIR:-sites}"
require_bench_dir "${SITES_DIR}"
CONNECTIONS="${RESTORE_CONNECTIONS:-4}"
AUTH="Authorization: ${RESTORE_SOURCE_TOKEN}"

echo "[restore] Checking site: ${RESTORE_SITE}"

if [[ ! -f "${SITES_DIR}/${RESTORE_SITE}/site_config.json" ]]; then
    echo "[restore] Site ${RESTORE_SITE} does not exist under ${SITES_DIR}/"
    echo "[restore] Create the site first"
    exit 1
fi

mkdir -p "${DOWNLOAD_DIR}"

echo "[restore] Fetching latest backup metadata from ${SOURCE_URL}"

DB_PATH="$(
    curl -fsS \
        -H "${AUTH}" \
        "${SOURCE_URL}/api/method/frappe.utils.backups.fetch_latest_backups" |
        jq -r '.message.database // empty'
)"

if [[ -z "${DB_PATH}" ]]; then
    echo "[restore] Source site returned no database backup"
    exit 1
fi

BACKUP_FILE="$(basename "${DB_PATH}")"
BACKUP_PATH="${DOWNLOAD_DIR}/${BACKUP_FILE}"

echo "[restore] Latest database backup: ${BACKUP_FILE}"

if [[ -f "${BACKUP_PATH}" && ! -f "${BACKUP_PATH}.aria2" ]]; then
    if gzip -t "${BACKUP_PATH}" 2>/dev/null; then
        echo "[restore] Backup already downloaded, skipping download"
        SKIP_DOWNLOAD=1
    else
        echo "[restore] Existing file is corrupt, re-downloading"
        rm -f "${BACKUP_PATH}"
    fi
fi

if [[ -z "${SKIP_DOWNLOAD:-}" ]]; then
    echo "[restore] Downloading backup into ${DOWNLOAD_DIR}"

    # exit 13 = file already fully downloaded, treat as success
    RC=0
    aria2c \
        --no-conf \
        -x "${CONNECTIONS}" \
        -s "${CONNECTIONS}" \
        -c \
        --auto-file-renaming=false \
        --allow-overwrite=false \
        --header="${AUTH}" \
        -d "${DOWNLOAD_DIR}" \
        -o "${BACKUP_FILE}" \
        "${SOURCE_URL}/backups/${BACKUP_FILE}" || RC=$?

    if [[ ${RC} -ne 0 && ${RC} -ne 13 ]]; then
        echo "[restore] aria2c failed (exit ${RC})"
        exit "${RC}"
    fi
fi

if [[ ! -f "${BACKUP_PATH}" ]]; then
    echo "[restore] Backup download failed"
    exit 1
fi

echo "[restore] Restoring into ${RESTORE_SITE}"
echo "[restore] WARNING: existing database data will be dropped"

bench --site "${RESTORE_SITE}" restore \
    "${BACKUP_PATH}" \
    --db-root-username "${DB_ROOT_USER}" \
    --db-root-password "${DB_ROOT_PASS}" \
    --force

if [[ -z "${RESTORE_SKIP_MIGRATE:-}" ]]; then
    echo "[restore] Migrating ${RESTORE_SITE}"
    bench --site "${RESTORE_SITE}" migrate
else
    echo "[restore] Skipping migrate (RESTORE_SKIP_MIGRATE set)"
fi

echo "[restore] Setting admin password"

bench --site "${RESTORE_SITE}" \
    set-admin-password "${RESTORE_ADMIN_PASSWORD}"

if [[ "${RESTORE_KEEP_BACKUP:-1}" == "0" ]]; then
    rm -f "${BACKUP_PATH}"
    echo "[restore] Backup deleted"
else
    echo "[restore] Backup kept at ${BACKUP_PATH} (delete manually when no longer needed)"
fi

echo "[restore] Done"
