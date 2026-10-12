#!/usr/bin/env bash
# Publish the built checkout as one commit on the orphan branch `release`,
# tagged v<package.json version>, and push both to origin.
#
# Run from the repo root after `npm ci && npm run build`. The release commit
# holds only what an install needs: dist/src, a package.json without scripts
# or devDependencies (so npm installs it from git with no build step),
# README.md and .env.example. Users install with:
#   npx m-fadil/frappe-executable#release <command>
#   npm i -g m-fadil/frappe-executable#v1.2.3
set -euo pipefail

BRANCH="${RELEASE_BRANCH:-release}"
REMOTE="${RELEASE_REMOTE:-origin}"

version="$(node -p 'require("./package.json").version')"
tag="v${version}"
source_sha="$(git rev-parse HEAD)"

[[ -f dist/src/cli.js ]] || { echo "dist/src/cli.js missing: run npm run build first" >&2; exit 1; }

# remote_has REF: 0 = exists, 1 = missing; any other git failure (network,
# auth) aborts rather than being mistaken for "missing"
remote_has() {
    local rc=0
    git ls-remote --exit-code "$REMOTE" "$1" >/dev/null || rc=$?
    case "$rc" in
        0) return 0 ;;
        2) return 1 ;;
        *) echo "git ls-remote ${REMOTE} $1 failed (exit ${rc})" >&2; exit "$rc" ;;
    esac
}

if remote_has "refs/tags/${tag}"; then
    echo "${tag} already exists on ${REMOTE}; bump version in package.json to release again" >&2
    exit 1
fi

wt="$(mktemp -d)"
trap 'git worktree remove --force "$wt" 2>/dev/null || rm -rf "$wt"' EXIT

if remote_has "refs/heads/${BRANCH}"; then
    git fetch --quiet --no-tags "$REMOTE" "$BRANCH"
    git worktree add --quiet -B "$BRANCH" "$wt" FETCH_HEAD
else
    # first release: a branch with no history shared with main
    git worktree add --quiet --detach "$wt"
    git -C "$wt" checkout --quiet --orphan "$BRANCH"
fi
# start from empty: nothing from main (or the previous release) survives
git -C "$wt" rm -rfq --ignore-unmatch .

mkdir -p "$wt/dist"
cp -r dist/src "$wt/dist/src"
cp README.md .env.example "$wt/"
node -e '
    const p = require("./package.json");
    delete p.scripts;
    delete p.devDependencies;
    require("fs").writeFileSync(process.argv[1], JSON.stringify(p, null, 2) + "\n");
' "$wt/package.json"

git -C "$wt" add -A
git -C "$wt" commit --quiet -m "release ${tag}" -m "source: ${source_sha}"
git -C "$wt" tag "$tag"
git -C "$wt" push --quiet "$REMOTE" "$BRANCH" "refs/tags/${tag}"
echo "published ${tag} on ${BRANCH} (source ${source_sha})"
