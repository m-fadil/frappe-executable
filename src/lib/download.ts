import { createReadStream, createWriteStream, existsSync, renameSync, rmSync, statSync } from 'node:fs';
import { Readable, Transform, Writable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import type { ReadableStream as WebReadableStream } from 'node:stream/web';
import { setTimeout as sleep } from 'node:timers/promises';
import { createGunzip } from 'node:zlib';
import { CliError } from './errors.js';

/** True when the whole file decompresses cleanly (`gzip -t`). */
export async function gzipIntact(path: string): Promise<boolean> {
    try {
        await pipeline(createReadStream(path), createGunzip(), new Writable({ write: (_chunk, _enc, cb) => cb() }));
        return true;
    } catch {
        return false;
    }
}

export interface DownloadOptions {
    headers?: Record<string, string>;
    log: (message: string) => void;
    /** total tries, counting the first; network errors, 408/429 and 5xx are retried */
    attempts?: number;
    /** first retry delay, doubling after each failure; default 1000 */
    backoffMs?: number;
    /** progress line on stderr; default: when stderr is a terminal */
    progress?: boolean;
}

const MiB = 1024 * 1024;

/**
 * Download `url` to `dest` through `dest.part`, renamed only on success, so an
 * interrupted run resumes where it stopped (HTTP Range) instead of starting
 * over, and a half-written `dest` never exists. A source that ignores Range is
 * re-downloaded from zero. On final failure the partial is kept for next time.
 */
export async function downloadResumable(url: string, dest: string, opts: DownloadOptions): Promise<void> {
    const part = `${dest}.part`;
    const attempts = opts.attempts ?? 4;
    const showProgress = opts.progress ?? Boolean(process.stderr.isTTY);

    // only failures count against `attempts`; restarting from zero does not
    for (let failures = 0; ;) {
        const have = existsSync(part) ? statSync(part).size : 0;
        if (have > 0) opts.log(`resuming from ${have} bytes`);

        let retryable = true;
        try {
            const res = await fetch(url, {
                headers: { ...opts.headers, ...(have > 0 ? { Range: `bytes=${have}-` } : {}) },
                redirect: 'follow',
            });

            let append = false;
            if (res.status === 206 && have > 0) {
                // only trust the resume when it starts exactly where our partial ends
                const start = /^bytes (\d+)-/.exec(res.headers.get('content-range') ?? '')?.[1];
                if (Number(start) !== have) {
                    await res.body?.cancel();
                    opts.log('source answered with a different range, restarting the download');
                    rmSync(part, { force: true });
                    continue;
                }
                append = true;
            } else if (res.status === 416 && have > 0) {
                await res.body?.cancel();
                // killed between the last byte and the rename: the partial is already whole
                const total = /^bytes \*\/(\d+)$/.exec(res.headers.get('content-range') ?? '')?.[1];
                if (Number(total) === have) {
                    renameSync(part, dest);
                    return;
                }
                opts.log('source rejected the resume range, restarting the download');
                rmSync(part, { force: true });
                continue;
            } else if (!res.ok || !res.body) {
                await res.body?.cancel();
                // the statuses curl --retry treats as transient
                retryable = [408, 429, 500, 502, 503, 504].includes(res.status);
                throw new CliError(`download failed: HTTP ${res.status} ${res.statusText} for ${url}`);
            } else if (res.status === 200 && have > 0) {
                opts.log('source does not support resume, restarting the download');
            }

            const length = Number(res.headers.get('content-length') ?? 0);
            const total = length > 0 ? length + (append ? have : 0) : 0;
            const meter = progressMeter(append ? have : 0, total, showProgress);
            try {
                await pipeline(
                    Readable.fromWeb(res.body as WebReadableStream<Uint8Array>),
                    meter.transform,
                    createWriteStream(part, { flags: append ? 'a' : 'w' }),
                );
            } finally {
                meter.done();
            }
            renameSync(part, dest);
            return;
        } catch (err) {
            const message = err instanceof Error ? err.message : String(err);
            failures++;
            if (!retryable || failures >= attempts) {
                if (existsSync(part)) opts.log(`partial kept at ${part}, rerun to resume`);
                throw err instanceof CliError ? err : new CliError(`download failed: ${message}`);
            }
            const wait = (opts.backoffMs ?? 1000) * 2 ** (failures - 1);
            opts.log(`${message} — retrying in ${wait / 1000}s (${failures}/${attempts - 1})`);
            await sleep(wait);
        }
    }
}

function progressMeter(start: number, total: number, enabled: boolean) {
    let seen = start;
    let last = 0;
    const draw = () => {
        const done = (seen / MiB).toFixed(1);
        const line =
            total > 0
                ? `${done} / ${(total / MiB).toFixed(1)} MiB (${Math.floor((seen / total) * 100)}%)`
                : `${done} MiB`;
        process.stderr.write(`\r  ${line}   `);
    };
    return {
        transform: new Transform({
            transform(chunk: Buffer, _enc, cb) {
                seen += chunk.length;
                const now = Date.now();
                if (enabled && now - last > 200) {
                    last = now;
                    draw();
                }
                cb(null, chunk);
            },
        }),
        done() {
            if (!enabled) return;
            draw();
            process.stderr.write('\n');
        },
    };
}
