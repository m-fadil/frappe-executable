import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, describe, test } from 'node:test';
import { checkBackupPath, listBackups, pathChoices } from '../src/lib/backups.js';

const root = mkdtempSync(join(tmpdir(), 'fe-bk-'));
after(() => rmSync(root, { recursive: true, force: true }));

/** Files with explicit mtimes (seconds), so ordering never depends on the clock. */
function tree(name: string, files: Record<string, number>): string {
    const dir = join(root, name);
    mkdirSync(dir);
    for (const [file, mtime] of Object.entries(files)) {
        writeFileSync(join(dir, file), 'x');
        utimesSync(join(dir, file), mtime, mtime);
    }
    return dir;
}

describe('listBackups', () => {
    test('newest first by the bench name timestamp, not mtime; mtime only for other names', () => {
        const dir = tree('list', {
            // copied without -p: the oldest backup has the newest mtime
            '20240101_010000-a-database.sql.gz': 1_900_000_000,
            '20250601_120000-a-database.sql.gz': 1_000_000_000,
            // no timestamp in the name: 2025-11-14 by mtime
            'manual.sql': 1_763_100_000,
            // not real times: fall back to mtime (2001), ties broken by name
            '20251399_000000-a-database.sql.gz': 1_000_000_000,
            '20250101_007000-a-database.sql.gz': 1_000_000_000,
            '20250601_120000-a-files.tar': 1_000_000_000,
            'site_config_backup.json': 1_000_000_000,
        });
        assert.deepEqual(
            listBackups(dir).map((b) => b.name),
            [
                'manual.sql',
                '20250601_120000-a-database.sql.gz',
                '20240101_010000-a-database.sql.gz',
                '20251399_000000-a-database.sql.gz',
                '20250101_007000-a-database.sql.gz',
            ],
        );
    });

    test('a missing directory has no backups', () => {
        assert.deepEqual(listBackups(join(root, 'nope')), []);
    });
});

describe('pathChoices', () => {
    const dir = tree('pick', { 'b.sql.gz': 1, 'notes.txt': 1, '.hidden.sql': 1 });
    mkdirSync(join(dir, 'archive'));
    mkdirSync(join(dir, '.cache'));
    symlinkSync(join(dir, 'archive'), join(dir, 'linked'));
    symlinkSync(join(dir, 'gone'), join(dir, 'dangling.sql'));
    const names = (term: string) => pathChoices(term, dir).map((c) => c.name);

    test('empty input offers the current directory first, then dirs before backups', () => {
        assert.deepEqual(names(''), ['./', 'archive/', 'linked/', 'b.sql.gz']);
        assert.equal(pathChoices('', dir)[0]!.value, dir);
    });

    test('completes the part after the last slash; hidden only when typed', () => {
        assert.deepEqual(names('a'), ['archive/']);
        assert.deepEqual(names('.'), ['.', '.cache/', '.hidden.sql']);
        assert.deepEqual(names('archive/'), ['archive/']);
        // the typed dir and its listed completion are one entry, not two
        assert.deepEqual(names('archive'), ['archive/']);
        assert.deepEqual(names('./archive'), ['./archive/']);
        assert.deepEqual(names(`${dir}/b`), [`${dir}/b.sql.gz`]);
    });

    test('an unusable typed path offers only completions', () => {
        assert.deepEqual(names('notes.txt'), []);
        assert.deepEqual(names('zzz/'), []);
    });
});

describe('checkBackupPath', () => {
    const dir = tree('check', { 'x.sql.gz': 1, 'x.tar': 1 });
    const empty = tree('check-empty', { 'x.tar': 1 });

    test('accepts a backup file or a directory holding one', () => {
        assert.equal(checkBackupPath(dir), true);
        assert.equal(checkBackupPath(join(dir, 'x.sql.gz')), true);
    });

    test('names what is wrong', () => {
        assert.match(String(checkBackupPath(empty)), /No \.sql \/ \.sql\.gz backups in/);
        assert.match(String(checkBackupPath(join(dir, 'x.tar'))), /is not a \.sql or \.sql\.gz backup/);
        assert.match(String(checkBackupPath(join(dir, 'missing'))), /does not exist/);
    });
});
