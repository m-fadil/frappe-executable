import { type Dirent, readdirSync, statSync, type Stats } from 'node:fs';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';

/** What `bench restore` takes as a database backup. */
export const isBackupName = (name: string): boolean => /\.sql(\.gz)?$/.test(name);

export interface Backup {
    path: string;
    name: string;
    /** From the `YYYYMMDD_HHMMSS-` prefix bench writes, else the file's mtime. */
    time: Date;
    size: number;
}

/** `~` and `~/x` as a shell would expand them, then made absolute. */
export function expandPath(path: string, cwd = process.cwd()): string {
    if (path === '~' || path.startsWith('~/')) return join(homedir(), path.slice(1));
    return resolve(cwd, path);
}

function statOrNull(path: string): Stats | null {
    try {
        return statSync(path);
    } catch {
        return null;
    }
}

/** Entries of `dir` with symlinks followed; unreadable dirs and dangling links are skipped. */
function entries(dir: string): Array<{ name: string; stat: Stats }> {
    let list: Dirent[];
    try {
        list = readdirSync(dir, { withFileTypes: true });
    } catch {
        return [];
    }
    return list.flatMap((e) => {
        const stat = statOrNull(join(dir, e.name));
        return stat ? [{ name: e.name, stat }] : [];
    });
}

/** The timestamp bench puts in front of backup names, read as local time. */
export function nameTime(name: string): Date | null {
    const m = /^(\d{4})(\d{2})(\d{2})_(\d{2})(\d{2})(\d{2})/.exec(name);
    if (!m) return null;
    const [y, mo, d, h, mi, s] = m.slice(1).map(Number) as [number, number, number, number, number, number];
    const time = new Date(y, mo - 1, d, h, mi, s);
    // reject 20261399_... or _007000: Date would silently roll them over
    const parts = [time.getMonth() + 1, time.getDate(), time.getHours(), time.getMinutes(), time.getSeconds()];
    return parts.every((v, i) => v === [mo, d, h, mi, s][i]) ? time : null;
}

/** Database backups directly inside `dir`, newest first. */
export function listBackups(dir: string): Backup[] {
    return entries(dir)
        .filter((e) => e.stat.isFile() && isBackupName(e.name))
        .map((e) => ({
            path: join(dir, e.name),
            name: e.name,
            time: nameTime(e.name) ?? e.stat.mtime,
            size: e.stat.size,
        }))
        .sort((a, b) => b.time.getTime() - a.time.getTime() || b.name.localeCompare(a.name));
}

export interface PathChoice {
    /** Absolute path. */
    value: string;
    /** Exactly what Tab writes into the input. */
    name: string;
    description?: string;
}

/**
 * Completions for a typed path: the path itself when it is usable, then the
 * directories and backups in its directory whose name starts with what follows
 * the last `/`. Hidden entries only show once the typed part starts with `.`.
 */
export function pathChoices(term: string, cwd = process.cwd()): PathChoice[] {
    const slash = term.lastIndexOf('/');
    const dirPart = term.slice(0, slash + 1);
    const base = term.slice(slash + 1);
    const dir = expandPath(dirPart || '.', cwd);

    const listed = entries(dir)
        .filter((e) => e.name.startsWith(base) && (base.startsWith('.') || !e.name.startsWith('.')))
        .filter((e) => e.stat.isDirectory() || (e.stat.isFile() && isBackupName(e.name)))
        .sort((a, b) => Number(b.stat.isDirectory()) - Number(a.stat.isDirectory()) || a.name.localeCompare(b.name))
        .map((e) => ({
            value: join(dir, e.name),
            name: `${dirPart}${e.name}${e.stat.isDirectory() ? '/' : ''}`,
        }));

    const self = expandPath(term || '.', cwd);
    const stat = statOrNull(self);
    const usable = stat?.isDirectory() || (stat?.isFile() && isBackupName(self));
    if (!usable || listed.some((c) => c.value === self)) return listed;
    const own = term === '' ? { name: './', description: 'current directory' } : { name: term };
    return [{ value: self, ...own }, ...listed];
}

/** Why `path` cannot be restored from, or true. */
export function checkBackupPath(path: string): true | string {
    const stat = statOrNull(path);
    if (!stat) return `${path} does not exist`;
    if (stat.isDirectory()) return listBackups(path).length > 0 || `No .sql / .sql.gz backups in ${path}`;
    return isBackupName(path) || `${path} is not a .sql or .sql.gz backup`;
}

const pad = (n: number) => String(n).padStart(2, '0');

export function formatBackup(b: Backup): string {
    const t = b.time;
    const date = `${t.getFullYear()}-${pad(t.getMonth() + 1)}-${pad(t.getDate())} ${pad(t.getHours())}:${pad(t.getMinutes())}`;
    const kb = b.size / 1024;
    const size =
        kb < 1024
            ? `${kb.toFixed(1)} KB`
            : kb < 1024 * 1024
              ? `${(kb / 1024).toFixed(1)} MB`
              : `${(kb / 1024 / 1024).toFixed(1)} GB`;
    return `${date}  ${b.name}  (${size})`;
}
