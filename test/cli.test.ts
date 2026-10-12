import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';

const CLI = fileURLToPath(new URL('../src/cli.js', import.meta.url));
const dir = mkdtempSync(join(tmpdir(), 'fe-cli-'));
after(() => rmSync(dir, { recursive: true, force: true }));

/** run the CLI with no terminal at all, as cron/CI would */
const cli = (args: string[], cwd = dir) => {
    const r = spawnSync(process.execPath, [CLI, ...args], {
        cwd,
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'pipe'],
        env: { PATH: process.env.PATH },
    });
    return { code: r.status, out: r.stdout, err: r.stderr };
};

const bench = (name: string, sites: string[]) => {
    const root = join(dir, name);
    for (const s of sites) {
        mkdirSync(join(root, 'sites', s), { recursive: true });
        writeFileSync(join(root, 'sites', s, 'site_config.json'), '{}');
    }
    return root;
};

describe('cli', () => {
    test('every command answers --help without a bench', () => {
        for (const c of ['bench-init', 'restore', 'sanitize', 'restore-sanitize', 'create-user']) {
            const r = cli([c, '--help']);
            assert.equal(r.code, 0, c);
            assert.match(r.out, new RegExp(`^Usage: frappe ${c} `), c);
        }
    });

    test('unknown command and bad option exit 2 with usage', () => {
        assert.equal(cli(['nope']).code, 2);
        const r = cli(['restore', '--nope']);
        assert.equal(r.code, 2);
        assert.match(r.err, /unknown option: --nope\n\nUsage: frappe restore/);
    });

    test('site commands refuse to run outside a bench', () => {
        const r = cli(['sanitize']);
        assert.equal(r.code, 1);
        assert.match(r.err, /not a bench directory/);
    });

    test('no terminal: fails fast naming what is missing instead of hanging', () => {
        const two = bench('two', ['a.localhost', 'b.localhost']);
        assert.match(cli(['restore'], two).err, /RESTORE_SITE is required \(2 sites here, no terminal/);

        const one = bench('one', ['a.localhost']);
        const r = cli(['restore', '-e', 'RESTORE_SOURCE_URL=https://x'], one);
        assert.match(r.out, /Site: a\.localhost/);
        assert.match(r.err, /RESTORE_SOURCE_TOKEN is required \(no terminal/);

        assert.match(cli(['create-user'], one).err, /USER_EMAIL is required/);
        assert.match(cli(['bench-init']).err, /MODE is required when running non-interactively/);
        assert.match(cli(['bench-init', '-e', 'MODE=server']).err, /no tty: set YES=1/);
        assert.match(
            cli(['bench-init', '-e', 'MODE=server', '-e', 'YES=1']).err,
            /missing required: DB_ROOT_PASS ADMIN_PASS/,
        );
        assert.match(cli(['bench-init', '-e', 'MODE=k8s', '-e', 'YES=1']).err, /MODE must be 'server' or 'docker'/);
    });
});
