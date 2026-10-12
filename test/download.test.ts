import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer, type IncomingMessage, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, describe, test } from 'node:test';
import { gzipSync } from 'node:zlib';
import { downloadResumable, gzipIntact } from '../src/lib/download.js';

const BODY = Buffer.from('0123456789'.repeat(1000));
const dir = mkdtempSync(join(tmpdir(), 'fe-dl-'));
let server: Server;
let base = '';
const seen: IncomingMessage[] = [];
let flakyDrops = 0;

before(async () => {
    server = createServer((req, res) => {
        seen.push(req);
        if (req.headers.authorization !== 'token k:s') return void res.writeHead(403).end();
        const range = /^bytes=(\d+)-$/.exec(req.headers.range ?? '');
        if (req.url === '/flaky' && flakyDrops > 0) {
            // drop the connection halfway through the body; how much of it reaches
            // the .part file before the error is timing, so the retry may or may not resume
            flakyDrops--;
            res.writeHead(200, { 'content-length': BODY.length });
            return void res.write(BODY.subarray(0, 5000), () => res.socket?.destroy());
        }
        if ((req.url === '/ranged' || req.url === '/flaky') && range) {
            const start = Number(range[1]);
            if (start >= BODY.length)
                return void res.writeHead(416, { 'content-range': `bytes */${BODY.length}` }).end();
            res.writeHead(206, {
                'content-length': BODY.length - start,
                'content-range': `bytes ${start}-${BODY.length - 1}/${BODY.length}`,
            });
            return void res.end(BODY.subarray(start));
        }
        if (req.url === '/ranged' || req.url === '/plain' || req.url === '/flaky') {
            res.writeHead(200, { 'content-length': BODY.length });
            return void res.end(BODY);
        }
        res.writeHead(404).end();
    });
    await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
    base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});
after(() => {
    server.close();
    rmSync(dir, { recursive: true, force: true });
});

const opts = { headers: { Authorization: 'token k:s' }, log: () => {}, progress: false, attempts: 1 };

describe('downloadResumable', () => {
    test('resumes a partial from where it stopped', async () => {
        const dest = join(dir, 'a.gz');
        writeFileSync(`${dest}.part`, BODY.subarray(0, 4000));
        seen.length = 0;
        await downloadResumable(`${base}/ranged`, dest, opts);
        assert.equal(seen[0]!.headers.range, 'bytes=4000-');
        assert.deepEqual(readFileSync(dest), BODY);
        assert.equal(existsSync(`${dest}.part`), false);
    });

    test('a partial that is already whole (416) is kept, not re-downloaded', async () => {
        const dest = join(dir, 'whole.gz');
        writeFileSync(`${dest}.part`, BODY);
        seen.length = 0;
        await downloadResumable(`${base}/ranged`, dest, opts);
        assert.equal(seen.length, 1);
        assert.deepEqual(readFileSync(dest), BODY);
    });

    test('a connection dropped mid-body is retried to a complete file', async () => {
        const dest = join(dir, 'flaky.gz');
        seen.length = 0;
        flakyDrops = 1;
        await downloadResumable(`${base}/flaky`, dest, { ...opts, attempts: 2, backoffMs: 0 });
        assert.equal(seen.length, 2);
        assert.deepEqual(readFileSync(dest), BODY);
        assert.equal(existsSync(`${dest}.part`), false);
    });

    test('gives up after `attempts` failures without leaving dest', async () => {
        const dest = join(dir, 'flaky-out.gz');
        seen.length = 0;
        flakyDrops = 2;
        await assert.rejects(
            downloadResumable(`${base}/flaky`, dest, { ...opts, attempts: 2, backoffMs: 0 }),
            /download failed/,
        );
        assert.equal(seen.length, 2);
        assert.equal(existsSync(dest), false);
    });

    test('restarts from zero when the source ignores Range', async () => {
        const dest = join(dir, 'b.gz');
        writeFileSync(`${dest}.part`, Buffer.from('garbage that must not survive'));
        await downloadResumable(`${base}/plain`, dest, opts);
        assert.deepEqual(readFileSync(dest), BODY);
    });

    test('restarts when the partial is already past the end (416)', async () => {
        const dest = join(dir, 'c.gz');
        writeFileSync(`${dest}.part`, Buffer.concat([BODY, Buffer.from('extra')]));
        await downloadResumable(`${base}/ranged`, dest, opts);
        assert.deepEqual(readFileSync(dest), BODY);
    });

    test('HTTP errors fail without leaving dest, and are not retried when 4xx', async () => {
        const dest = join(dir, 'd.gz');
        seen.length = 0;
        await assert.rejects(downloadResumable(`${base}/missing`, dest, { ...opts, attempts: 3 }), /HTTP 404/);
        assert.equal(seen.length, 1);
        assert.equal(existsSync(dest), false);
        await assert.rejects(downloadResumable(`${base}/plain`, dest, { ...opts, headers: {} }), /HTTP 403/);
    });
});

describe('gzipIntact', () => {
    test('accepts a whole archive, rejects a truncated one', async () => {
        const gz = gzipSync(BODY);
        const good = join(dir, 'good.gz');
        const cut = join(dir, 'cut.gz');
        writeFileSync(good, gz);
        writeFileSync(cut, gz.subarray(0, gz.length - 10));
        assert.equal(await gzipIntact(good), true);
        assert.equal(await gzipIntact(cut), false);
    });
});
