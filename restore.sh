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

ENV_HELP="Anything required and unset is asked for on the terminal, so no env file is
needed. Passwords are read without echo.

Variables:
  RESTORE_SITE            asked; auto-picked when the bench has one site
  RESTORE_SOURCE_URL      asked
  RESTORE_SOURCE_TOKEN    asked  Authorization header value
  DB_ROOT_USER            asked  default: root
  DB_ROOT_PASS            asked  database root password (alias: DB_PASSWORD)
  RESTORE_ADMIN_PASSWORD  asked  admin password to set after restore
  RESTORE_DOWNLOAD_DIR    default: \$PWD
  RESTORE_SITES_DIR       default: sites
  RESTORE_CONNECTIONS     default: 4        aria2c -x/-s; ignored by the curl fallback
  RESTORE_KEEP_BACKUP     default: 1        set to 0 to delete after restore
  RESTORE_SKIP_MIGRATE    default: empty    non-empty skips 'bench migrate'"

parse_args "$@"

# DB_ROOT_PASS is the shared name (bench-init.sh uses it too); DB_PASSWORD is
# kept as an alias so older env files keep working.
DB_ROOT_PASS="${DB_ROOT_PASS:-${DB_PASSWORD:-}}"

SITES_DIR="${RESTORE_SITES_DIR:-sites}"
require_bench_dir "${SITES_DIR}"

for dep in curl jq; do
    command -v "$dep" >/dev/null 2>&1 || die "[restore] missing dependency: ${dep}"
done

# anything still unset is asked for, so this runs fine with no env file
pick_site   RESTORE_SITE "${SITES_DIR}"
ask_var     RESTORE_SOURCE_URL     "Source site URL (https://...)"
ask_secret  RESTORE_SOURCE_TOKEN   "Source Authorization header (e.g. 'token key:secret')"
ask_var     DB_ROOT_USER           "Database root user" root
ask_secret  DB_ROOT_PASS           "Database root password"
ask_secret  RESTORE_ADMIN_PASSWORD "Admin password to set after restore"

SOURCE_URL="${RESTORE_SOURCE_URL%/}"
DOWNLOAD_DIR="${RESTORE_DOWNLOAD_DIR:-$PWD}"
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
BACKUP_URL="${SOURCE_URL}/backups/${BACKUP_FILE}"

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

download() {
    local rc=0

    if command -v aria2c >/dev/null 2>&1; then
        echo "[restore] Downloading with aria2c (${CONNECTIONS} connections) into ${DOWNLOAD_DIR}"

        # exit 13 = file already fully downloaded, treat as success
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
            "${BACKUP_URL}" || rc=$?

        [[ ${rc} -eq 0 || ${rc} -eq 13 ]] && return 0
        echo "[restore] aria2c failed (exit ${rc})"
        return "${rc}"
    fi

    echo "[restore] aria2c not found, falling back to curl (single connection) into ${DOWNLOAD_DIR}"

    # download to .part so an interrupted run is resumable: a bare partial file
    # is indistinguishable from a corrupt complete one, and gets deleted above
    local part="${BACKUP_PATH}.part"
    [[ -f "${part}" ]] && echo "[restore] resuming from $(wc -c <"${part}") bytes"

    curl -fL --retry 3 -C - -H "${AUTH}" -o "${part}" "${BACKUP_URL}" || rc=$?

    # 33 = source refuses byte ranges, so resuming is impossible; start clean
    if [[ ${rc} -eq 33 ]]; then
        echo "[restore] source does not support resume, restarting the download"
        rm -f "${part}"
        rc=0
        curl -fL --retry 3 -H "${AUTH}" -o "${part}" "${BACKUP_URL}" || rc=$?
    fi

    if [[ ${rc} -ne 0 ]]; then
        echo "[restore] curl failed (exit ${rc})"
        [[ -f "${part}" ]] && echo "[restore] partial kept at ${part}, rerun to resume"
        return "${rc}"
    fi

    mv -f "${part}" "${BACKUP_PATH}"
}

if [[ -z "${SKIP_DOWNLOAD:-}" ]]; then
    download
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
