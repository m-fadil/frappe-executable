# frappe-executable

One `frappe` command for the Frappe chores that are too fiddly to remember and
too boring to write twice: spinning up a bench, cloning production down to a
dev box without spamming your customers, and minting API users.

No hardcoded hosts, passwords or site names. Every knob is an environment
variable, settable inline (`-e KEY=val`) or from a file (`--env-file .env`),
and anything missing is asked for with arrow-key / masked prompts
([`@inquirer/prompts`](https://www.npmjs.com/package/@inquirer/prompts)).

## Install

Needs Node.js `^20.17`, `^22.13` or `>=23.5`, npm, and `git` (npm resolves the
GitHub ref with it).

Run without installing:

```bash
npx m-fadil/frappe-executable#release restore      # newest release
npx m-fadil/frappe-executable#v1.0.0 restore       # a specific version
```

Or install globally:

```bash
npm install -g m-fadil/frappe-executable#release   # or #v1.0.0 to pin
```

A normal global npm package: it lands in your npm prefix
(`npm prefix -g`) with its own `node_modules`, and `frappe` goes on `PATH`
next to every other global bin. Re-run the line to upgrade.

`#release` is a branch holding only prebuilt releases, one commit per version,
each tagged `vX.Y.Z`; nothing is compiled on your machine. `main` is the source
and is not installable as-is.

If `npm prefix -g` is root-owned (a system Node), either prefix the line with
`sudo`, or move the prefix somewhere you own, once:

```bash
npm config set prefix ~/.npm-global      # then add ~/.npm-global/bin to PATH
```

From a checkout:

```bash
npm ci && npm pack && npm install -g ./frappe-executable-*.tgz
```

`npm install -g .` also works, but links the checkout instead of copying it,
so `dist/` must be built (`npm run build`) and stays tied to the working tree.

Upgrading from the old shell-script install (`/opt/frappe-executable`)? Run
the old `frappe uninstall` first, so its `/usr/local/bin/frappe` link does not
shadow the new one.

### Uninstall

```bash
frappe uninstall          # = npm uninstall -g frappe-executable, in the prefix it is installed in
```

It finds its own prefix, so an nvm or custom-prefix install needs no
arguments. It refuses to run from a git checkout — that is yours to `rm`.

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
frappe --version              # the installed version
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
where Tab completes the highlighted entry and Enter takes it if it is a backup
or a directory holding some (otherwise the entry is copied into the input so
you can go deeper). `.gz` backups are checked for corruption before anything
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
variables grouped at the top. It ships in the package too:
`$(npm root -g)/frappe-executable/.env.example`.

## Layout

```
src/cli.ts                  dispatcher: frappe <command>, --version, uninstall
src/commands/*.ts           one file per command, each exports a Command
src/lib/env.ts              -h / -e / --env-file parsing and precedence
src/lib/envfile.ts          the env file syntax above
src/lib/prompt.ts           @inquirer/prompts wrappers, ask-only-when-unset
src/lib/proc.ts             spawn helpers (bench, aria2c)
src/lib/download.ts         resumable download, gzip integrity check
src/lib/bench.ts            bench dir check, site listing / picking
test/*.test.ts              node:test suites; npm test
.github/workflows/          CI on Node 20/22/24; release on a version bump
.github/scripts/            publish-release.sh: build → `release` branch + tag
```

Adding a command is: write `src/commands/foo.ts` exporting a `Command`
(name, one-line `desc`, `about`, `envHelp`, `run(env)`), and add it to
`COMMANDS` in `src/cli.ts`.

Formatting is Prettier (`.prettierrc.json`): `npm run format` to apply,
`npm run format:check` is what CI runs.

Releasing is a version bump on `main`:

```bash
npm version patch            # or minor / major; bumps package.json + lock, no commit or tag (.npmrc)
git commit -am "release v$(node -p 'require("./package.json").version')"
git push
```

The `release` workflow sees the new version, runs the format check and tests,
then `.github/scripts/publish-release.sh` commits the build to the `release`
branch (message records the source commit) and tags it `vX.Y.Z`. Do not tag
yourself: tags belong to release commits, and pushing a version that is already
tagged is a no-op. The workflow can also be re-run by hand from the Actions tab.
