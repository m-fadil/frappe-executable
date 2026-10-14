import { existsSync, mkdirSync, rmSync, statSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { checkBackupPath, expandPath, formatBackup, listBackups, pathChoices } from '../lib/backups.js';
import { pickSite, requireBenchDir, siteExists } from '../lib/bench.js';
import { type Command, logger } from '../lib/command.js';
import { downloadResumable, gzipIntact } from '../lib/download.js';
import type { Env } from '../lib/env.js';
import { CliError } from '../lib/errors.js';
import { run, spawnWait, which } from '../lib/proc.js';
import { askSecret, askVar, canPrompt, promptSearch, promptSelect } from '../lib/prompt.js';

const log = logger('restore');

async function latestBackupPath(sourceUrl: string, auth: string): Promise<string> {
    const url = `${sourceUrl}/api/method/frappe.utils.backups.fetch_latest_backups`;
    let res: Response;
    try {
        res = await fetch(url, { headers: { Authorization: auth } });
    } catch (err) {
        throw new CliError(`[restore] cannot reach ${url}: ${(err as Error).message}`);
    }
    if (!res.ok) throw new CliError(`[restore] ${url} answered HTTP ${res.status} ${res.statusText}`);
    let body: { message?: { database?: unknown } };
    try {
        body = (await res.json()) as typeof body;
    } catch {
        throw new CliError(`[restore] ${url} did not return JSON`);
    }
    const db = body.message?.database;
    if (typeof db !== 'string' || db === '') throw new CliError('[restore] Source site returned no database backup');
    return db;
}

/** aria2c with N connections; exit 13 means "already fully downloaded". */
async function downloadAria2(env: Env, url: string, auth: string, dir: string, file: string, connections: string) {
    log(`Downloading with aria2c (${connections} connections) into ${dir}`);
    const { code } = await spawnWait(env, 'aria2c', [
        '--no-conf',
        '-x',
        connections,
        '-s',
        connections,
        '-c',
        '--auto-file-renaming=false',
        '--allow-overwrite=false',
        `--header=${auth}`,
        '-d',
        dir,
        '-o',
        file,
        url,
    ]);
    if (code !== 0 && code !== 13) throw new CliError(`[restore] aria2c failed (exit ${code})`, code);
}

/** Latest backup of the source site, downloaded (or resumed) into RESTORE_DOWNLOAD_DIR. */
async function downloadLatest(env: Env, sourceUrl: string, token: string): Promise<string> {
    const downloadDir = resolve(env.or('RESTORE_DOWNLOAD_DIR', process.cwd()));
    const connections = env.or('RESTORE_CONNECTIONS', '4');
    mkdirSync(downloadDir, { recursive: true });

    log(`Fetching latest backup metadata from ${sourceUrl}`);
    const backupFile = basename(await latestBackupPath(sourceUrl, token));
    const backupPath = join(downloadDir, backupFile);
    const backupUrl = `${sourceUrl}/backups/${backupFile}`;
    log(`Latest database backup: ${backupFile}`);

    if (existsSync(backupPath) && !existsSync(`${backupPath}.aria2`)) {
        if (await gzipIntact(backupPath)) {
            log('Backup already downloaded, skipping download');
            return backupPath;
        }
        log('Existing file is corrupt, re-downloading');
        rmSync(backupPath, { force: true });
    }

    if (which('aria2c')) {
        await downloadAria2(env, backupUrl, `Authorization: ${token}`, downloadDir, backupFile, connections);
    } else {
        log(`aria2c not found, downloading over a single connection into ${downloadDir}`);
        await downloadResumable(backupUrl, backupPath, { headers: { Authorization: token }, log: (m) => log(m) });
    }
    if (!existsSync(backupPath)) throw new CliError('[restore] Backup download failed');
    return backupPath;
}

/** A file is used as is; a directory offers its backups newest first (no terminal: the newest). */
async function pickLocalBackup(from: string): Promise<string> {
    const path = expandPath(from);
    const problem = checkBackupPath(path);
    if (problem !== true) throw new CliError(`[restore] RESTORE_FROM: ${problem}`);
    if (!statSync(path).isDirectory()) return path;

    const backups = listBackups(path);
    const newest = backups[0]!;
    if (backups.length === 1 || !canPrompt()) {
        log(`Newest of ${backups.length} backup(s) in ${path}: ${newest.name}`);
        return newest.path;
    }
    return promptSelect(
        `Backup in ${path} (newest first)`,
        backups.map((b) => ({ value: b.path, name: formatBackup(b) })),
        newest.path,
    );
}

type Source = { local: string } | { url: string; token: string };

/** RESTORE_FROM wins over RESTORE_SOURCE_URL; with neither set, ask which. */
async function chooseSource(env: Env): Promise<Source> {
    if (!env.filled('RESTORE_FROM') && !env.filled('RESTORE_SOURCE_URL')) {
        if (!canPrompt()) {
            throw new CliError(
                'RESTORE_SOURCE_URL or RESTORE_FROM is required (no terminal to ask on; pass -e NAME=... or --env-file)',
            );
        }
        const mode = await promptSelect('Restore from', [
            { value: 'download', name: 'Download the latest backup from a source site' },
            { value: 'local', name: 'A local backup file, or a directory of backups' },
        ]);
        if (mode === 'local') {
            env.set(
                'RESTORE_FROM',
                await promptSearch('Backup file or directory (Tab completes)', pathChoices, checkBackupPath),
            );
        }
    }
    if (env.filled('RESTORE_FROM')) return { local: await pickLocalBackup(env.get('RESTORE_FROM')!) };

    const url = (await askVar(env, 'RESTORE_SOURCE_URL', 'Source site URL (https://...)')).replace(/\/+$/, '');
    const token = await askSecret(env, 'RESTORE_SOURCE_TOKEN', "Source Authorization header (e.g. 'token key:secret')");
    return { url, token };
}

async function restoreRun(env: Env): Promise<void> {
    // DB_ROOT_PASS is the shared name (bench-init uses it too); DB_PASSWORD is
    // kept as an alias so older env files keep working.
    if (!env.filled('DB_ROOT_PASS') && env.filled('DB_PASSWORD')) env.set('DB_ROOT_PASS', env.get('DB_PASSWORD')!);

    const sitesDir = env.or('RESTORE_SITES_DIR', 'sites');
    requireBenchDir(sitesDir);

    // anything still unset is asked for, so this runs fine with no env file
    const site = await pickSite(env, 'RESTORE_SITE', sitesDir);
    const source = await chooseSource(env);
    const dbUser = await askVar(env, 'DB_ROOT_USER', 'Database root user', 'root');
    const dbPass = await askSecret(env, 'DB_ROOT_PASS', 'Database root password');
    const adminPass = await askSecret(env, 'RESTORE_ADMIN_PASSWORD', 'Admin password to set after restore');

    log(`Checking site: ${site}`);
    if (!siteExists(sitesDir, site)) {
        throw new CliError(`[restore] Site ${site} does not exist under ${sitesDir}/\n[restore] Create the site first`);
    }

    let backupPath: string;
    if ('url' in source) {
        backupPath = await downloadLatest(env, source.url, source.token);
    } else {
        backupPath = source.local;
        log(`Using local backup: ${backupPath}`);
        if (backupPath.endsWith('.gz')) {
            log('Checking backup integrity');
            if (!(await gzipIntact(backupPath)))
                throw new CliError(`[restore] ${backupPath} is corrupt (gzip check failed)`);
        }
    }

    log(`Restoring into ${site}`);
    log('WARNING: existing database data will be dropped');
    await run(env, 'bench', [
        '--site',
        site,
        'restore',
        backupPath,
        '--db-root-username',
        dbUser,
        '--db-root-password',
        dbPass,
        '--force',
    ]);

    if (env.filled('RESTORE_SKIP_MIGRATE')) {
        log('Skipping migrate (RESTORE_SKIP_MIGRATE set)');
    } else {
        log(`Migrating ${site}`);
        await run(env, 'bench', ['--site', site, 'migrate']);
    }

    log('Setting admin password');
    await run(env, 'bench', ['--site', site, 'set-admin-password', adminPass]);

    // a local backup is the user's file: never delete it
    if ('url' in source) {
        if (env.or('RESTORE_KEEP_BACKUP', '1') === '0') {
            rmSync(backupPath, { force: true });
            log('Backup deleted');
        } else {
            log(`Backup kept at ${backupPath} (delete manually when no longer needed)`);
        }
    }
    log('Done');
}

export const restore: Command = {
    name: 'restore',
    desc: 'Restore the latest backup from a source site or a local path',
    about: `Restore a database backup into a local site: either the latest backup of a
source site (downloaded) or a local .sql / .sql.gz file or directory of them.`,
    envHelp: `Anything required and unset is asked for on the terminal, so no env file is
needed. Passwords are read without echo. With neither RESTORE_FROM nor
RESTORE_SOURCE_URL set, you are asked which source to use.

Variables:
  RESTORE_SITE            asked; auto-picked when the bench has one site
  RESTORE_FROM            local .sql/.sql.gz file, or a directory to pick from
                          (newest first; no terminal: the newest). Wins over
                          RESTORE_SOURCE_URL. Never deleted.
  RESTORE_SOURCE_URL      asked unless RESTORE_FROM is set
  RESTORE_SOURCE_TOKEN    asked unless RESTORE_FROM is set; Authorization header value
  DB_ROOT_USER            asked  default: root
  DB_ROOT_PASS            asked  database root password (alias: DB_PASSWORD)
  RESTORE_ADMIN_PASSWORD  asked  admin password to set after restore
  RESTORE_DOWNLOAD_DIR    default: current directory
  RESTORE_SITES_DIR       default: sites
  RESTORE_CONNECTIONS     default: 4        aria2c -x/-s; unused without aria2c
  RESTORE_KEEP_BACKUP     default: 1        set to 0 to delete a downloaded backup after restore
  RESTORE_SKIP_MIGRATE    default: empty    non-empty skips 'bench migrate'`,
    run: restoreRun,
};
