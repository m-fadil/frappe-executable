import { existsSync, mkdirSync, rmSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { pickSite, requireBenchDir, siteExists } from '../lib/bench.js';
import { logger } from '../lib/command.js';
import { downloadResumable, gzipIntact } from '../lib/download.js';
import { CliError } from '../lib/errors.js';
import { run, spawnWait, which } from '../lib/proc.js';
import { askSecret, askVar } from '../lib/prompt.js';
const log = logger('restore');
async function latestBackupPath(sourceUrl, auth) {
    const url = `${sourceUrl}/api/method/frappe.utils.backups.fetch_latest_backups`;
    let res;
    try {
        res = await fetch(url, { headers: { Authorization: auth } });
    }
    catch (err) {
        throw new CliError(`[restore] cannot reach ${url}: ${err.message}`);
    }
    if (!res.ok)
        throw new CliError(`[restore] ${url} answered HTTP ${res.status} ${res.statusText}`);
    let body;
    try {
        body = (await res.json());
    }
    catch {
        throw new CliError(`[restore] ${url} did not return JSON`);
    }
    const db = body.message?.database;
    if (typeof db !== 'string' || db === '')
        throw new CliError('[restore] Source site returned no database backup');
    return db;
}
/** aria2c with N connections; exit 13 means "already fully downloaded". */
async function downloadAria2(env, url, auth, dir, file, connections) {
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
    if (code !== 0 && code !== 13)
        throw new CliError(`[restore] aria2c failed (exit ${code})`, code);
}
async function restoreRun(env) {
    // DB_ROOT_PASS is the shared name (bench-init uses it too); DB_PASSWORD is
    // kept as an alias so older env files keep working.
    if (!env.filled('DB_ROOT_PASS') && env.filled('DB_PASSWORD'))
        env.set('DB_ROOT_PASS', env.get('DB_PASSWORD'));
    const sitesDir = env.or('RESTORE_SITES_DIR', 'sites');
    requireBenchDir(sitesDir);
    // anything still unset is asked for, so this runs fine with no env file
    const site = await pickSite(env, 'RESTORE_SITE', sitesDir);
    const sourceUrl = (await askVar(env, 'RESTORE_SOURCE_URL', 'Source site URL (https://...)')).replace(/\/+$/, '');
    const token = await askSecret(env, 'RESTORE_SOURCE_TOKEN', "Source Authorization header (e.g. 'token key:secret')");
    const dbUser = await askVar(env, 'DB_ROOT_USER', 'Database root user', 'root');
    const dbPass = await askSecret(env, 'DB_ROOT_PASS', 'Database root password');
    const adminPass = await askSecret(env, 'RESTORE_ADMIN_PASSWORD', 'Admin password to set after restore');
    const downloadDir = resolve(env.or('RESTORE_DOWNLOAD_DIR', process.cwd()));
    const connections = env.or('RESTORE_CONNECTIONS', '4');
    const auth = `Authorization: ${token}`;
    log(`Checking site: ${site}`);
    if (!siteExists(sitesDir, site)) {
        throw new CliError(`[restore] Site ${site} does not exist under ${sitesDir}/\n[restore] Create the site first`);
    }
    mkdirSync(downloadDir, { recursive: true });
    log(`Fetching latest backup metadata from ${sourceUrl}`);
    const backupFile = basename(await latestBackupPath(sourceUrl, token));
    const backupPath = join(downloadDir, backupFile);
    const backupUrl = `${sourceUrl}/backups/${backupFile}`;
    log(`Latest database backup: ${backupFile}`);
    let skipDownload = false;
    if (existsSync(backupPath) && !existsSync(`${backupPath}.aria2`)) {
        if (await gzipIntact(backupPath)) {
            log('Backup already downloaded, skipping download');
            skipDownload = true;
        }
        else {
            log('Existing file is corrupt, re-downloading');
            rmSync(backupPath, { force: true });
        }
    }
    if (!skipDownload) {
        if (which('aria2c')) {
            await downloadAria2(env, backupUrl, auth, downloadDir, backupFile, connections);
        }
        else {
            log(`aria2c not found, downloading over a single connection into ${downloadDir}`);
            await downloadResumable(backupUrl, backupPath, {
                headers: { Authorization: token },
                log: (m) => log(m),
            });
        }
    }
    if (!existsSync(backupPath))
        throw new CliError('[restore] Backup download failed');
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
    }
    else {
        log(`Migrating ${site}`);
        await run(env, 'bench', ['--site', site, 'migrate']);
    }
    log('Setting admin password');
    await run(env, 'bench', ['--site', site, 'set-admin-password', adminPass]);
    if (env.or('RESTORE_KEEP_BACKUP', '1') === '0') {
        rmSync(backupPath, { force: true });
        log('Backup deleted');
    }
    else {
        log(`Backup kept at ${backupPath} (delete manually when no longer needed)`);
    }
    log('Done');
}
export const restore = {
    name: 'restore',
    desc: 'Pull the latest backup from a source site and restore it',
    about: 'Pull the latest database backup from a source site and restore it locally.',
    envHelp: `Anything required and unset is asked for on the terminal, so no env file is
needed. Passwords are read without echo.

Variables:
  RESTORE_SITE            asked; auto-picked when the bench has one site
  RESTORE_SOURCE_URL      asked
  RESTORE_SOURCE_TOKEN    asked  Authorization header value
  DB_ROOT_USER            asked  default: root
  DB_ROOT_PASS            asked  database root password (alias: DB_PASSWORD)
  RESTORE_ADMIN_PASSWORD  asked  admin password to set after restore
  RESTORE_DOWNLOAD_DIR    default: current directory
  RESTORE_SITES_DIR       default: sites
  RESTORE_CONNECTIONS     default: 4        aria2c -x/-s; unused without aria2c
  RESTORE_KEEP_BACKUP     default: 1        set to 0 to delete after restore
  RESTORE_SKIP_MIGRATE    default: empty    non-empty skips 'bench migrate'`,
    run: restoreRun,
};
