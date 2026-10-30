# frappe-executable

One `frappe` command for the Frappe chores that are too fiddly to remember and
too boring to write twice: spinning up a bench, cloning production down to a
dev box without spamming your customers, and minting API users.

No hardcoded hosts, passwords or site names. Every knob is an environment
variable, settable inline (`-e KEY=val`) or from a file (`--env-file .env`),
and anything missing is asked for with arrow-key / masked prompts.

## Install

A single static Linux binary (x86_64 and aarch64, musl). No Node, no runtime
dependencies; `aria2c` is optional and only speeds up downloads.

```bash
curl -fsSL https://github.com/m-fadil/frappe-executable/releases/latest/download/install.sh | sh
```

That downloads the release archive for your architecture, verifies its SHA-256
checksum, and installs `frappe` into `~/.local/bin`. Options:

```bash
FRAPPE_VERSION=v2.0.0 sh install.sh        # a specific version (or: sh -s -- v2.0.0)
FRAPPE_INSTALL_DIR=/usr/local/bin sh install.sh
```

Upgrade by running the installer again. Each release also publishes
`SHA256SUMS` and a build provenance attestation.

From a checkout:

```bash
cargo build --release        # -> target/release/frappe
```

Coming from v1 (npm)? Run the old `frappe uninstall` (or
`npm uninstall -g frappe-executable`) first so it does not shadow the new
binary. Commands, environment variables and exit codes are unchanged.

### Update

```bash
frappe update             # latest stable release
frappe update --check     # only say whether one exists
frappe update v2.0.1      # a specific version (also a downgrade)
```

It runs that release's `install.sh`, so the SHA-256 check and the in-place
replace are the same as a fresh install. Needs `sh` and `curl` or `wget`.
It refuses to run from a cargo `target/` directory.

### Uninstall

```bash
frappe uninstall          # removes the running binary
```

It refuses to run from a cargo `target/` directory — that is yours to `rm`.

## The one thing to know

Commands act on the bench in your **current directory**, never on wherever
the package happens to be installed. So:

```bash
cd ~/frappe-bench
frappe sanitize .env
```

Run one outside a bench and it says so instead of guessing.
`bench-init` is the exception — it _creates_ a bench in the current directory.

## Commands

```
frappe                        # list commands
frappe <command> --help       # every variable it reads, with defaults
frappe --version, -v          # the installed version
frappe update                 # update to the latest release
frappe uninstall              # remove itself
```

| Command            | What it does                                                              |
| ------------------ | ------------------------------------------------------------------------- |
| `bench-init`       | Init a bench + site, `server` or `docker` layout                          |
| `restore`          | Restore a DB backup (latest from a source site, or a local path), migrate |
| `sanitize`         | Disable every outbound integration on a restored site                     |
| `restore-sanitize` | The two above, in that order                                              |
| `create-user`      | Create a user, print `api_key:api_secret`                                 |

### bench-init

Asks `server` or `docker` first, because that choice changes the flags:

- **docker** — `bench init --skip-redis-config-generation`, writes the
  `redis_cache` / `redis_queue` / `redis_socketio` global config, and creates
  the site with `--mariadb-user-host-login-scope=%`. Defaults to
  `DB_HOST=mariadb`. This is the devcontainer / compose layout.
- **server** — none of that. Bench generates its own redis config and
  supervisor entries, `DB_HOST=127.0.0.1`.

Then it asks for the two passwords (no defaults) and shows a menu with every
value; pick one to change it, or **Apply & run**, which confirms once more
before anything runs. DB Name follows Site Name until you give it a value of
its own.

```bash
frappe bench-init                                    # pick mode, edit in the menu
frappe bench-init -e MODE=docker                     # skip the mode picker
frappe bench-init --env-file bench.env -e YES=1      # fully unattended
```

### restore + sanitize

`restore` asks the source site for its latest backup over
`fetch_latest_backups`, downloads it, then restores + migrates + resets the
admin password. An already-downloaded archive is reused if it decompresses
cleanly, so a rerun costs you nothing.

The download uses `aria2c` when it is installed — 4 connections, resumable,
tune with `RESTORE_CONNECTIONS`. Without it, a built-in single-connection
download takes over: it writes to `<backup>.part` and renames on success, so an
interrupted download resumes where it stopped (HTTP Range) instead of starting
over, retries transient failures, and keeps the partial on a final failure on
purpose. A source that ignores Range is simply re-downloaded. Nothing else to
install; `aria2c` is just faster on a fat pipe.

Already have the backup? Point `RESTORE_FROM` at a `.sql` / `.sql.gz` file, or
at a directory of them, and nothing is downloaded. A directory offers its
backups newest first (by the `YYYYMMDD_HHMMSS-` bench puts in the name, else
the file's mtime); with no terminal the newest is taken. `RESTORE_FROM` wins
over `RESTORE_SOURCE_URL`, and a local backup is never deleted, whatever
`RESTORE_KEEP_BACKUP` says. With neither set, `restore` asks which source to
use; picking "local" gives a path prompt that starts at the current directory,
where Tab completes the highlighted entry and Enter takes the typed path if it
is a backup or a directory holding some (otherwise it says what is wrong). `.gz` backups are checked for corruption before anything
is dropped.

`sanitize` is the part you do not skip. A fresh production clone will happily
send real invoices to real customers, so it: mutes the email queue, disables
every Email Account, Webhook and Notification, parks Auto Repeat, empties the
queue, and forces `enabled = 0` on the integration singles you list. Tables
that do not exist on the site are skipped, not fatal. It finishes by
re-counting everything still enabled, so you get proof rather than a promise.

```bash
cd ~/frappe-bench
frappe restore-sanitize .env
frappe restore-sanitize -e RESTORE_FROM=~/backups    # newest local backup, no download
frappe sanitize .env -e SANITIZE_SINGLES=            # skip the singles step
```

### create-user

```bash
frappe create-user -e USER_EMAIL=mcp@example.com
# → mcp@example.com, first name "mcp", System Manager, prints key:secret

frappe create-user \
  -e USER_EMAIL=ops@example.com -e USER_FIRST_NAME=Ops -e USER_LAST_NAME=Team \
  -e USER_ROLES='Accounts User,Sales User' -e USER_API_KEY=0
```

Roles are checked against the site before the insert, so a typo fails loudly
instead of silently creating a user with no permissions. An existing user is
reused untouched — only the API credentials are fetched or generated. The
credentials are the last line of output, so `| tail -1` works; prompts are
drawn on stderr and stay visible.

## No env file needed

Every command runs with nothing configured — anything required that is unset is
asked for on the terminal, and passwords are masked:

```
$ cd ~/frappe-bench && frappe restore
Site: dev.localhost                                    # auto-picked, one site here
✔ Source site URL (https://...) https://prod.example.com
✔ Source Authorization header (e.g. 'token key:secret') ••••••••••••
✔ Database root user root
✔ Database root password ••••••••
✔ Admin password to set after restore ••••••
[restore] Checking site: dev.localhost
```

A bench with several sites gives you an arrow-key picker instead. Ctrl+C at
any prompt stops the command (exit 130) without touching anything.

Prompts only fire for what is still unset, so an env file, `-e`, and typing are
freely mixable — handy for keeping the boring parts in a file and typing only
the password:

```bash
frappe restore prod.env          # everything but the passwords in the file
```

With no terminal to ask on (cron, CI) it fails fast and names the variable
rather than hanging:

```
RESTORE_SITE is required (2 sites here, no terminal to ask on; pass -e RESTORE_SITE=... or --env-file)
```

## Options, everywhere

Same three on every command:

```
-h, --help              variables and defaults for that command
-e VAR=val              repeatable; also -eVAR=val, --env VAR=val, --env=VAR=val
    --env-file PATH     repeatable; also --env-file=PATH, or a bare path
```

Later `--env-file` beats earlier ones, `-e` beats every file regardless of
argument order, and files beat the process environment.

```bash
frappe restore base.env prod.env -e RESTORE_KEEP_BACKUP=0
```

Env files are `KEY=value` lines with shell-style rules: `#` comments (a
trailing one needs whitespace before it), optional `export`, `'single'` quotes
taken literally, `"double"` quotes with `\"` `\\` `\$` escapes, and `$VAR` /
`${VAR}` expanded from earlier lines and the environment. A value with spaces
must be quoted — unquoted whitespace is an error with the file and line, not a
silent truncation. Nothing in the file is ever executed.

Start from [`.env.example`](.env.example) — one section per command, shared
variables grouped at the top. It ships in every release archive too.

## Layout

```
src/main.rs                 dispatcher: frappe <command>, --version, update, uninstall
src/commands/*.rs           one file per command, each exports a COMMAND
src/env.rs                  -h / -e / --env-file parsing and precedence
src/envfile.rs              the env file syntax above
src/prompt.rs               inquire wrappers, ask-only-when-unset
src/proc.rs                 spawn helpers (bench, aria2c), SIGINT handling
src/download.rs             resumable download, gzip integrity check
src/backups.rs              backup listing, path completion
src/bench.rs                bench dir check, site listing / picking
tests/cli.rs                end-to-end CLI tests; unit tests sit beside the code
install.sh                  release installer
.github/workflows/          test on push/PR; release on a v*.*.* tag
```

Adding a command is: write `src/commands/foo.rs` exporting a
`pub static COMMAND: Command` (name, one-line `desc`, `about`, `env_help`,
`run`), and add it to `COMMANDS` in `src/commands/mod.rs`.

Checks, as CI runs them: `cargo fmt --check`,
`cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`.

Releasing: bump `version` in `Cargo.toml` (and `Cargo.lock`), merge, then push
the matching tag:

```bash
git tag v2.0.0 && git push origin v2.0.0
```

The `release` workflow checks the tag equals the crate version, runs fmt,
clippy and tests, builds both musl targets, and publishes the archives,
checksums, provenance and `install.sh` to a GitHub release (a tag containing
`-` is a prerelease). It can also be re-run by hand from the Actions tab on a
tag.
