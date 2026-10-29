#!/bin/sh
# Installer for frappe: downloads the release binary for this OS/architecture,
# verifies its checksum, and installs it into ~/.local/bin.
#
#   curl -fsSL https://github.com/m-fadil/frappe-executable/releases/latest/download/install.sh | sh
#   curl -fsSL https://github.com/m-fadil/frappe-executable/releases/download/vX.Y.Z/install.sh | sh
#
# Version choice (first match wins): FRAPPE_VERSION, argument (`sh -s -- vX.Y.Z`),
# the version baked in at release time, then the latest stable release.
#
#   FRAPPE_VERSION       version to install, e.g. vX.Y.Z or X.Y.Z-rc.1
#   FRAPPE_INSTALL_DIR   target directory (default: $HOME/.local/bin)
#   FRAPPE_RELEASES_URL  releases base URL (default: GitHub Releases; for testing)

set -eu

REPO="m-fadil/frappe-executable"
# Filled in by the release workflow; empty means the latest stable release.
DEFAULT_VERSION=""

die() {
    printf 'install.sh: %s\n' "$*" >&2
    exit 1
}

have() {
    command -v "$1" >/dev/null 2>&1
}

fetch() {
    if have curl; then
        curl -fsSL --retry 3 -o "$2" "$1"
    elif have wget; then
        wget -q -O "$2" "$1"
    else
        die "curl or wget is required."
    fi
}

# Prints the final URL of a redirect.
redirect_target() {
    if have curl; then
        curl -fsSLI -o /dev/null -w '%{url_effective}' "$1"
    elif have wget; then
        # wget 1.x prints a Location header; wget2 prints "URL '...' not followed".
        wget -S --spider --max-redirect=0 "$1" 2>&1 |
            sed -n -e 's/^ *[Ll]ocation: *//p' -e "s/^URL '\(.*\)' not followed.*/\1/p" | tr -d '\r' | sed -n '1p'
    else
        die "curl or wget is required."
    fi
}

detect_target() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$arch" in
        x86_64 | amd64) arch="x86_64" ;;
        aarch64 | arm64) arch="aarch64" ;;
        *) die "unsupported architecture: $arch" ;;
    esac
    case "$os" in
        Linux) printf '%s-unknown-linux-musl' "$arch" ;;
        *) die "unsupported operating system: $os (releases are Linux only)" ;;
    esac
}

resolve_version() {
    version="${FRAPPE_VERSION:-${1:-$DEFAULT_VERSION}}"
    if [ -z "$version" ]; then
        location=$(redirect_target "$releases/latest") || location=""
        version="${location##*/tag/}"
        case "$location" in
            */tag/v*) ;;
            *) die "no stable release found. Pick a version: FRAPPE_VERSION=vX.Y.Z" ;;
        esac
    fi
    case "$version" in
        v*) ;;
        *) version="v$version" ;;
    esac
    printf '%s' "$version"
}

verify() {
    if have sha256sum; then
        sha256sum -c "$1"
    elif have shasum; then
        shasum -a 256 -c "$1"
    else
        die "sha256sum or shasum is required to verify the download."
    fi
}

main() {
    releases="${FRAPPE_RELEASES_URL:-https://github.com/$REPO/releases}"
    install_dir="${FRAPPE_INSTALL_DIR:-${HOME:?HOME is not set}/.local/bin}"

    target=$(detect_target)
    version=$(resolve_version "${1:-}")
    name="frappe-$version-$target"
    archive="$name.tar.gz"

    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT INT TERM

    printf 'Installing frappe %s (%s)\n' "$version" "$target"
    fetch "$releases/download/$version/$archive" "$tmp/$archive" ||
        die "failed to download $releases/download/$version/$archive (no such version or platform?)"
    fetch "$releases/download/$version/$archive.sha256" "$tmp/$archive.sha256" ||
        die "failed to download checksum $archive.sha256"
    (cd "$tmp" && verify "$archive.sha256" >/dev/null) || die "checksum mismatch for $archive."

    tar -xzf "$tmp/$archive" -C "$tmp" || die "failed to extract $archive."
    [ -f "$tmp/$name/frappe" ] || die "archive does not contain frappe."

    mkdir -p "$install_dir"
    cp "$tmp/$name/frappe" "$install_dir/.frappe.new"
    chmod 755 "$install_dir/.frappe.new"
    mv -f "$install_dir/.frappe.new" "$install_dir/frappe"

    printf 'Installed: %s/frappe\n' "$install_dir"
    # shellcheck disable=SC2016 # $PATH must be shown literally
    case ":$PATH:" in
        *":$install_dir:"*) ;;
        *) printf 'Add to PATH: export PATH="%s:$PATH"\n' "$install_dir" ;;
    esac
}

main "$@"
