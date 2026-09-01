# frappe-executable

Five shell scripts for the Frappe chores that are too fiddly to remember and
too boring to write twice: spinning up a bench, cloning production down to a
dev box without spamming your customers, and minting API users.

No hardcoded hosts, passwords or site names. Every knob is an environment
variable, settable inline (`-e KEY=val`) or from a file (`--env-file .env`).

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/m-fadil/frappe-executable/main/install.sh | bash
```

Drops the scripts in `/opt/frappe-executable` and links `frappe` into `/usr/local/bin`.

**Do not run it as `sudo curl ... | bash`** — that elevates `curl`, not `bash`,
which is not what you want. The script elevates itself instead: it checks
whether the target paths are writable, and only then calls `sudo` for the
copy and the symlink. The download and unpack always run as you. It asks for
the password before downloading anything, so you never get a half-install,
and if there is no terminal to ask on it tells you the two ways out instead of
dying mid-run.

If you would rather elevate the whole thing yourself, `curl ... | sudo bash`
works — just note that `$HOME` is then root's, so the rootless overrides below
will not do what you expect.

Rootless, no `sudo` anywhere:

```bash
curl -fsSL https://raw.githubusercontent.com/m-fadil/frappe-executable/main/install.sh \
  | INSTALL_DIR=~/.local/share/frappe-executable BIN_DIR=~/.local/bin bash
```

Pin a tag or commit with `REF=v1.0.0`, rename the command with
`BIN_NAME=fe`. Re-run the same line to upgrade.

### Uninstall

```bash
frappe uninstall
```

Works out where it lives on its own, so a custom `INSTALL_DIR` / `BIN_DIR` /
`BIN_NAME` needs no arguments. It removes every symlink that resolves to the
installed entry — including ones you made by hand, and ones outside `PATH` —
then the scripts, then the directory itself. Anything you left in there (an
`.env`, say) is kept and reported rather than deleted, and it refuses outright
on a directory that has no `frappe-exec.sh` in it. `sudo` only if the paths
need it.

Equivalent without an install, or if the entry is already gone:

```bash
curl -fsSL .../install.sh | bash -s -- --uninstall
curl -fsSL .../install.sh | INSTALL_DIR=~/.local/share/frappe-executable bash -s -- --uninstall
```

Running it against a git checkout is refused — that is yours to `rm`.

Or just clone it — the scripts run fine from a checkout, no install step
required.

Needs `bash`, `curl` and `tar` to install. `restore` also needs `jq`, and
uses `aria2c` if you have it — see below.

## The one thing to know

Commands act on the bench in your **current directory**, never on wherever
the scripts happen to be installed. So:

```bash
cd ~/frappe-bench
frappe sanitize .env
```

Run one outside a bench and it says so instead of guessing.
`bench-init` is the exception — it *creates* a bench in the current directory.

## Commands

```
frappe                        # list commands
frappe <command> --help       # every variable it reads, with defaults
frappe --version              # the ref that was installed
frappe uninstall              # remove itself
```

| Command | What it does |
| --- | --- |
| `bench-init` | Init a bench + site, `server` or `docker` layout |
| `restore` | Pull the latest DB backup from a source site, restore, migrate |
| `sanitize` | Disable every outbound integration on a restored site |
| `restore-sanitize` | The two above, in that order |
| `create-user` | Create a user, print `api_key:api_secret` |

### bench-init

Asks `server` or `docker` first, because that choice changes the flags:

- **docker** — `bench init --skip-redis-config-generation`, writes the
  `redis_cache` / `redis_queue` / `redis_socketio` global config, and creates
  the site with `--mariadb-user-host-login-scope=%`. Defaults to
  `DB_HOST=mariadb`. This is the devcontainer / compose layout.
- **server** — none of that. Bench generates its own redis config and
  supervisor entries, `DB_HOST=127.0.0.1`.

Then a TUI to review the config before anything runs. Passwords have no
defaults; it refuses to run until you set them.

```bash
frappe bench-init                                    # pick mode, edit in the TUI
frappe bench-init -e MODE=docker                     # skip the mode picker
frappe bench-init --env-file bench.env -e YES=1      # fully unattended
```

### restore + sanitize

`restore` asks the source site for its latest backup over
`fetch_latest_backups`, downloads it, then restores + migrates + resets the
admin password. An already-downloaded archive is reused if `gzip -t` says it is
intact, so a rerun costs you nothing.

The download uses `aria2c` when it is installed — 4 connections, resumable,
tune with `RESTORE_CONNECTIONS`. Without it, it falls back to `curl`: one
connection, still resumable. The curl path writes to `<backup>.part` and
renames on success, so an interrupted download is picked up where it stopped
instead of starting over, and a failed one leaves the partial behind on
purpose. No extra install needed either way; `aria2c` is just faster on a fat
pipe.

`sanitize` is the part you do not skip. A fresh production clone will happily
send real invoices to real customers, so it: mutes the email queue, disables
every Email Account, Webhook and Notification, parks Auto Repeat, empties the
queue, and forces `enabled = 0` on the integration singles you list. Tables
that do not exist on the site are skipped, not fatal. It finishes by
re-counting everything still enabled, so you get proof rather than a promise.

```bash
cd ~/frappe-bench
frappe restore-sanitize .env
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
credentials are the last line of output, so `| tail -1` works.

## Options, everywhere

Same three on every command:

```
-h, --help              variables and defaults for that command
-e VAR=val              repeatable; also -eVAR=val and --env VAR=val
    --env-file PATH     repeatable; also --env-file=PATH, or a bare path
```

Later `--env-file` beats earlier ones, and `-e` beats every file regardless of
argument order. Env files are plain `KEY=value`, sourced by bash, so `#`
comments and quoting work as you would expect.

```bash
frappe restore base.env prod.env -e RESTORE_KEEP_BACKUP=0
```

Start from [`.env.example`](.env.example) — one section per script, shared
variables grouped at the top.

## Layout

```
frappe-exec.sh        dispatcher; finds subcommands by their '# desc:' line
lib.sh                the shared 60 lines: --help, -e, --env-file, bench check
bench-init.sh         \
restore.sh             |
sanitize.sh            |  subcommands, also runnable directly
restore-sanitize.sh    |
create-user.sh        /
install.sh            curl | bash installer, and the uninstaller
test_lib.sh           asserts for the argument parser; ./test_lib.sh
```

Adding a subcommand is: drop in a `foo.sh`, source `lib.sh`, give it a
`# desc:` comment on line 2. The dispatcher picks it up, no registry to edit.
