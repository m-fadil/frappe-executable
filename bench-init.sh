#!/usr/bin/env bash
# desc: Init a new bench + site (server or docker layout)
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

USAGE="Usage: ${FE_CMD:-$(basename "$0")} [options] [env-file...]

Interactive Frappe bench init.

Options:
  -h, --help              Show this help
  -e VAR=val, --env       Set/override a variable (repeatable)
      --env-file PATH     Load variables from an env file (repeatable)"

ENV_HELP="Variables:
  MODE                    server | docker (prompted when unset)
  YES                     non-empty: skip the TUI and run immediately
  FRAPPE_VERSION          default: version-16
  BENCH_NAME              default: frappe-bench
  SITE_NAME               default: development.localhost
  DB_HOST                 default: mariadb (docker) / 127.0.0.1 (server)
  DB_PORT                 default: 3306
  DB_NAME                 default: derived from SITE_NAME
  DB_ROOT_USER            default: root
  DB_ROOT_PASS            required (no default)
  ADMIN_PASS              required (no default)
  REDIS_CACHE             docker only, default: redis://redis-cache:6379
  REDIS_QUEUE             docker only, default: redis://redis-queue:6379
  REDIS_SOCKETIO          docker only, default: redis://redis-socketio:6379
  BENCH_INIT_EXTRA        extra args appended to 'bench init'
  BENCH_NEW_SITE_EXTRA    extra args appended to 'bench new-site'

Docker mode adds --skip-redis-config-generation to 'bench init', writes the
redis_* global config, and creates the site with
--mariadb-user-host-login-scope=%. Server mode does none of those."

parse_args "$@"

# =========================
# CLEANUP & TRAP
# =========================
# only touch the terminal when we actually own one
INTERACTIVE=1
[[ -n "${YES:-}" ]] && INTERACTIVE=0
[[ -t 0 && -t 1 ]] || INTERACTIVE=0

cleanup() {
    (( INTERACTIVE )) || return 0
    tput cnorm 2>/dev/null || true
    tput rmcup 2>/dev/null || true
    echo
}
trap cleanup EXIT
trap 'cleanup; echo "Error pada baris $LINENO. Exiting."; exit 1' ERR

# =========================
# READ KEY (dipakai mode picker + menu)
# =========================
# Output: UP, DOWN, ENTER, ESC, atau karakternya sendiri
read_key() {
    local char char2 char3
    IFS= read -r -s -n1 char
    if [[ "$char" == $'\x1b' ]]; then
        IFS= read -r -s -n1 -t 0.1 char2 || { echo "ESC"; return; }
        IFS= read -r -s -n1 -t 0.1 char3 || { echo "ESC"; return; }
        if [[ "$char2" == "[" ]]; then
            case "$char3" in
                A) echo "UP" ;;
                B) echo "DOWN" ;;
                *) echo "ESC" ;;
            esac
        else
            echo "ESC"
        fi
    elif [[ "$char" == "" || "$char" == $'\r' ]]; then
        echo "ENTER"
    else
        echo "$char"
    fi
}

# =========================
# MODE PICKER
# =========================
pick_mode() {
    local C_RESET='\e[0m' C_HEADER='\e[1;36m' C_SEL='\e[1;33;44m' C_NORMAL='\e[0;37m' C_INFO='\e[0;90m'
    local selected=0
    local names=(server docker)
    local descs=(
        "bench mengelola redis sendiri, DB di 127.0.0.1"
        "redis & mariadb sebagai service terpisah (devcontainer)"
    )

    while true; do
        clear
        printf "${C_HEADER}╔══════════════════════════════════════════════════════════╗\n"
        printf "║              🧭  Pilih Target Deployment                 ║\n"
        printf "╚══════════════════════════════════════════════════════════╝${C_RESET}\n\n"

        local i
        for i in "${!names[@]}"; do
            if [[ "$i" -eq "$selected" ]]; then
                printf "  ${C_SEL} ▶ %-52s ${C_RESET}\n" "${names[$i]}"
            else
                printf "    ${C_NORMAL}%-52s${C_RESET}\n" "${names[$i]}"
            fi
            printf "      ${C_INFO}%s${C_RESET}\n\n" "${descs[$i]}"
        done

        case "$(read_key)" in
            UP)    selected=$(( (selected - 1 + ${#names[@]}) % ${#names[@]} )) ;;
            DOWN)  selected=$(( (selected + 1) % ${#names[@]} )) ;;
            ENTER) printf '%s' "${names[$selected]}" >&3; return 0 ;;
            ESC|q|Q) return 0 ;;   # nothing on fd3 -> caller keluar
        esac
    done
}

# =========================
# MODE + DEFAULT CONFIG
# =========================
if (( INTERACTIVE )); then
    tput smcup 2>/dev/null || true
    tput civis 2>/dev/null || true
fi

if [[ -z "${MODE:-}" ]]; then
    (( INTERACTIVE )) || die "MODE is required when running non-interactively (-e MODE=server|docker)"
    MODE="$( { pick_mode 3>&1 1>&2; } )"
    [[ -z "$MODE" ]] && { cleanup; exit 0; }
fi

case "$MODE" in
    server|docker) ;;
    *) cleanup; die "MODE must be 'server' or 'docker' (got: ${MODE})" ;;
esac

FRAPPE_VERSION="${FRAPPE_VERSION:-version-16}"
BENCH_NAME="${BENCH_NAME:-frappe-bench}"
SITE_NAME="${SITE_NAME:-development.localhost}"
DB_PORT="${DB_PORT:-3306}"
DB_ROOT_USER="${DB_ROOT_USER:-root}"
DB_NAME="${DB_NAME:-$(printf '%s' "$SITE_NAME" | tr -c 'a-zA-Z0-9' '_')}"
DB_ROOT_PASS="${DB_ROOT_PASS:-}"
ADMIN_PASS="${ADMIN_PASS:-}"

LABELS=("Mode" "Frappe Version" "Bench Name" "Site Name" "DB Host" "DB Port" "DB Name" "DB Root User" "DB Root Password" "Admin Password")
KEYS=(MODE FRAPPE_VERSION BENCH_NAME SITE_NAME DB_HOST DB_PORT DB_NAME DB_ROOT_USER DB_ROOT_PASS ADMIN_PASS)

if [[ "$MODE" == "docker" ]]; then
    DB_HOST="${DB_HOST:-mariadb}"
    REDIS_CACHE="${REDIS_CACHE:-redis://redis-cache:6379}"
    REDIS_QUEUE="${REDIS_QUEUE:-redis://redis-queue:6379}"
    REDIS_SOCKETIO="${REDIS_SOCKETIO:-redis://redis-socketio:6379}"
    LABELS+=("Redis Cache" "Redis Queue" "Redis SocketIO")
    KEYS+=(REDIS_CACHE REDIS_QUEUE REDIS_SOCKETIO)
else
    DB_HOST="${DB_HOST:-127.0.0.1}"
fi

SECRET_KEYS=(DB_ROOT_PASS ADMIN_PASS)
REQUIRED_KEYS=(DB_ROOT_PASS ADMIN_PASS)

MENU_ITEMS=${#LABELS[@]}
IDX_APPLY=${#LABELS[@]}
IDX_QUIT=$(( ${#LABELS[@]} + 1 ))
TOTAL_ITEMS=$(( ${#LABELS[@]} + 2 ))

# =========================
# HELPERS
# =========================
in_list() {
    local needle="$1"; shift
    local x
    for x in "$@"; do [[ "$x" == "$needle" ]] && return 0; done
    return 1
}

is_secret()   { in_list "$1" "${SECRET_KEYS[@]}"; }
is_required() { in_list "$1" "${REQUIRED_KEYS[@]}"; }

missing_required() {
    local k out=()
    for k in "${REQUIRED_KEYS[@]}"; do
        [[ -z "${!k:-}" ]] && out+=("$k")
    done
    printf '%s' "${out[*]-}"
}

get_display_value() {
    local key="$1" val="${!1:-}"
    if [[ -z "$val" ]]; then
        echo "<belum diisi>"
    elif is_secret "$key"; then
        echo "••••••••"
    else
        echo "$val"
    fi
}

# =========================
# DRAW MENU
# =========================
draw_menu() {
    local selected="$1"
    local label_width=22
    local val_width=35

    local C_RESET='\e[0m'
    local C_HEADER='\e[1;36m'
    local C_SEL='\e[1;33;44m'
    local C_LABEL='\e[0;37m'
    local C_VAL='\e[1;32m'
    local C_MISS='\e[1;31m'
    local C_ACTION='\e[1;35m'
    local C_BOX='\e[0;34m'

    clear
    printf "${C_HEADER}"
    printf "╔══════════════════════════════════════════════════════════╗\n"
    printf "║         🛠  Frappe Bench Init  (mode: %-18s║\n" "${MODE})"
    printf "╚══════════════════════════════════════════════════════════╝\n"
    printf "${C_RESET}"
    printf "  ${C_BOX}▲/▼${C_RESET} navigasi   ${C_BOX}ENTER${C_RESET} pilih   ${C_BOX}Q${C_RESET} keluar\n"
    printf "\n"

    local i val color
    for i in "${!LABELS[@]}"; do
        val=$(get_display_value "${KEYS[$i]}")
        color="$C_VAL"
        [[ -z "${!KEYS[$i]:-}" ]] && color="$C_MISS"
        if [[ "$i" -eq "$selected" ]]; then
            printf "  ${C_SEL} ▶ %-${label_width}s  %-${val_width}s ${C_RESET}\n" "${LABELS[$i]}" "$val"
        else
            printf "    ${C_LABEL}%-${label_width}s${C_RESET}  ${color}%s${C_RESET}\n" "${LABELS[$i]}" "$val"
        fi
    done

    printf "\n"
    printf "  ${C_BOX}──────────────────────────────────────────────────────${C_RESET}\n"

    if [[ "$selected" -eq "$IDX_APPLY" ]]; then
        printf "  ${C_SEL} ▶ %-55s ${C_RESET}\n" "✅  Apply & Run"
    else
        printf "    ${C_ACTION}%-56s${C_RESET}\n" "✅  Apply & Run"
    fi

    if [[ "$selected" -eq "$IDX_QUIT" ]]; then
        printf "  ${C_SEL} ▶ %-55s ${C_RESET}\n" "❌  Quit"
    else
        printf "    ${C_ACTION}%-56s${C_RESET}\n" "❌  Quit"
    fi

    printf "\n"
}

# =========================
# EDIT VALUE
# =========================
edit_value() {
    local label="$1"
    local var_name="$2"
    local current="${!var_name:-}"
    local secret=false
    is_secret "$var_name" && secret=true

    local C_RESET='\e[0m' C_HEADER='\e[1;36m' C_PROMPT='\e[1;33m'
    local C_INFO='\e[0;37m' C_OK='\e[1;32m' C_WARN='\e[1;31m'

    clear
    printf "${C_HEADER}╔══════════════════════════════════════════════════════════╗\n"
    printf "║  Edit: %-50s║\n" "$label"
    printf "╚══════════════════════════════════════════════════════════╝${C_RESET}\n\n"

    if [[ "$secret" == "true" ]]; then
        printf "  ${C_INFO}Nilai saat ini : ••••••••${C_RESET}\n"
    else
        printf "  ${C_INFO}Nilai saat ini : ${C_OK}%s${C_RESET}\n" "$current"
    fi

    [[ "$var_name" == "MODE" ]] && printf "  ${C_INFO}Pilihan valid  : server | docker (restart script)${C_RESET}\n"

    printf "\n  ${C_INFO}ESC = batal │ ENTER = simpan${C_RESET}\n\n"
    printf "  ${C_PROMPT}Nilai baru : ${C_RESET}"

    local input="" char

    tput cnorm 2>/dev/null || true

    while true; do
        IFS= read -r -s -n1 char

        if [[ "$char" == $'\x1b' ]]; then
            IFS= read -r -s -n2 -t 0.05 _ 2>/dev/null || true
            printf "\n\n  ${C_WARN}Batal.${C_RESET}\n"
            sleep 0.5
            tput civis 2>/dev/null || true
            return
        fi

        if [[ "$char" == "" || "$char" == $'\r' ]]; then
            break
        fi

        if [[ "$char" == $'\x7f' || "$char" == $'\b' ]]; then
            if [[ ${#input} -gt 0 ]]; then
                input="${input%?}"
                printf '\b \b'
            fi
            continue
        fi

        input+="$char"
        if [[ "$secret" == "true" ]]; then
            printf '•'
        else
            printf '%s' "$char"
        fi
    done

    printf "\n"

    if [[ -z "$input" ]]; then
        printf "\n  ${C_WARN}Input kosong — nilai tidak diubah.${C_RESET}\n"
        sleep 0.8
        tput civis 2>/dev/null || true
        return
    fi

    if [[ "$var_name" == "MODE" && "$input" != "server" && "$input" != "docker" ]]; then
        printf "\n  ${C_WARN}MODE harus 'server' atau 'docker'.${C_RESET}\n"
        sleep 1
        tput civis 2>/dev/null || true
        return
    fi

    printf -v "$var_name" '%s' "$input"
    printf "\n  ${C_OK}✔ Tersimpan.${C_RESET}\n"
    sleep 0.5
    tput civis 2>/dev/null || true

    if [[ "$var_name" == "MODE" ]]; then
        printf "  ${C_WARN}MODE berubah — jalankan ulang script agar default ikut berubah.${C_RESET}\n"
        sleep 1.5
    fi
}

# =========================
# CONFIRM DIALOG
# =========================
confirm_run() {
    local C_RESET='\e[0m' C_HEADER='\e[1;36m' C_SEL='\e[1;33;44m'
    local C_NORMAL='\e[0;37m' C_OK='\e[1;32m' C_WARN='\e[1;31m'

    local missing
    missing="$(missing_required)"
    if [[ -n "$missing" ]]; then
        clear
        printf "${C_WARN}  Belum lengkap: %s${C_RESET}\n\n  Isi dulu sebelum menjalankan.\n" "$missing"
        sleep 2
        return 1
    fi

    local selected=0
    local options=("Ya, jalankan sekarang" "Tidak, kembali ke menu")

    while true; do
        clear
        printf "${C_HEADER}╔══════════════════════════════════════════════════════════╗\n"
        printf "║               ⚠  Konfirmasi Eksekusi                    ║\n"
        printf "╚══════════════════════════════════════════════════════════╝${C_RESET}\n\n"

        printf "  Konfigurasi yang akan digunakan:\n\n"
        local i
        for i in "${!LABELS[@]}"; do
            printf "  ${C_OK}%-22s${C_RESET}: %s\n" "${LABELS[$i]}" "$(get_display_value "${KEYS[$i]}")"
        done
        printf "\n"

        for i in "${!options[@]}"; do
            if [[ "$i" -eq "$selected" ]]; then
                printf "  ${C_SEL} ▶ %-52s ${C_RESET}\n" "${options[$i]}"
            else
                printf "    ${C_NORMAL}${options[$i]}${C_RESET}\n"
            fi
        done
        printf "\n"

        case "$(read_key)" in
            UP)   selected=$(( (selected - 1 + 2) % 2 )) ;;
            DOWN) selected=$(( (selected + 1) % 2 )) ;;
            ENTER) return "$selected" ;;
            ESC|q|Q) return 1 ;;
        esac
    done
}

# =========================
# RUN BENCH
# =========================
run_bench() {
    tput cnorm 2>/dev/null || true
    clear

    local C_RESET='\e[0m' C_HEADER='\e[1;36m' C_OK='\e[1;32m' C_STEP='\e[1;33m'

    printf "${C_HEADER}╔══════════════════════════════════════════════════════════╗\n"
    printf "║              🚀 Menjalankan Bench Init                   ║\n"
    printf "╚══════════════════════════════════════════════════════════╝${C_RESET}\n\n"

    local init_args=(--ignore-exist --frappe-branch="$FRAPPE_VERSION" --dev)
    local site_args=(
        --db-name="$DB_NAME"
        --db-root-username="$DB_ROOT_USER"
        --db-root-password="$DB_ROOT_PASS"
        --admin-password="$ADMIN_PASS"
    )

    if [[ "$MODE" == "docker" ]]; then
        init_args+=(--skip-redis-config-generation)
        site_args+=(--mariadb-user-host-login-scope=%)
    fi

    # shellcheck disable=SC2206
    [[ -n "${BENCH_INIT_EXTRA:-}" ]] && init_args+=(${BENCH_INIT_EXTRA})
    # shellcheck disable=SC2206
    [[ -n "${BENCH_NEW_SITE_EXTRA:-}" ]] && site_args+=(${BENCH_NEW_SITE_EXTRA})

    printf "${C_STEP}[1/3] Inisialisasi bench...${C_RESET}\n"
    bench init "${init_args[@]}" "$BENCH_NAME"

    cd "$BENCH_NAME"

    printf "\n${C_STEP}[2/3] Set konfigurasi global...${C_RESET}\n"
    bench set-config -g db_host "$DB_HOST"
    bench set-config -g db_port "$DB_PORT"
    if [[ "$MODE" == "docker" ]]; then
        bench set-config -g redis_cache    "$REDIS_CACHE"
        bench set-config -g redis_queue    "$REDIS_QUEUE"
        bench set-config -g redis_socketio "$REDIS_SOCKETIO"
    else
        printf "  (mode server: redis config dibuat oleh bench)\n"
    fi

    printf "\n${C_STEP}[3/3] Membuat site baru...${C_RESET}\n"
    bench new-site "${site_args[@]}" "$SITE_NAME"

    printf "\n${C_OK}✔ Setup selesai!${C_RESET}\n\n"
}

# =========================
# NON-INTERACTIVE
# =========================
if [[ -n "${YES:-}" ]]; then
    missing="$(missing_required)"
    [[ -n "$missing" ]] && die "missing required: $missing"
    run_bench
    exit 0
fi

# =========================
# MAIN LOOP
# =========================
(( INTERACTIVE )) || die "no tty: set YES=1 to run non-interactively"

selected=0

while true; do
    draw_menu "$selected"

    case "$(read_key)" in
        UP)   (( selected = (selected - 1 + TOTAL_ITEMS) % TOTAL_ITEMS )) ;;
        DOWN) (( selected = (selected + 1) % TOTAL_ITEMS )) ;;
        ENTER)
            if [[ "$selected" -lt "$MENU_ITEMS" ]]; then
                edit_value "${LABELS[$selected]}" "${KEYS[$selected]}"
            elif [[ "$selected" -eq "$IDX_APPLY" ]]; then
                if confirm_run; then
                    tput rmcup 2>/dev/null || true
                    run_bench
                    exit 0
                fi
            elif [[ "$selected" -eq "$IDX_QUIT" ]]; then
                break
            fi
            ;;
        q|Q|ESC) break ;;
    esac
done

echo "  Keluar."
