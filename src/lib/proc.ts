import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { accessSync, constants, statSync } from 'node:fs';
import { delimiter, join } from 'node:path';
import type { Env } from './env.js';
import { CliError } from './errors.js';

export interface RunOptions {
    cwd?: string;
    /** extra variables for this child only */
    extraEnv?: Record<string, string>;
    /** collect stdout instead of passing it through */
    capture?: boolean;
}

export interface RunResult {
    code: number;
    stdout: string;
}

/**
 * Spawn and wait. stdin/stderr are always the terminal's. A child killed by
 * Ctrl+C rejects with exit 130 so the whole command stops, like a shell script.
 * Error messages name only the program: args can hold passwords.
 */
export async function spawnWait(
    env: Env,
    cmd: string,
    args: readonly string[],
    opts: RunOptions = {},
): Promise<RunResult> {
    const child = spawn(cmd, args, {
        cwd: opts.cwd,
        env: env.toProcessEnv(opts.extraEnv),
        stdio: ['inherit', opts.capture ? 'pipe' : 'inherit', 'inherit'],
    });
    const chunks: Buffer[] = [];
    child.stdout?.on('data', (c: Buffer) => chunks.push(c));

    // Ctrl+C reaches the child too (same process group); outlive it like a
    // shell would, so its cleanup output lands before we exit
    const ignoreSigint = () => {};
    process.on('SIGINT', ignoreSigint);
    let code: number | null;
    let signal: NodeJS.Signals | null;
    try {
        [code, signal] = (await once(child, 'close')) as [number | null, NodeJS.Signals | null];
    } catch (err) {
        throw new CliError(`cannot run ${cmd}: ${(err as Error).message}`);
    } finally {
        process.off('SIGINT', ignoreSigint);
    }
    if (signal === 'SIGINT') throw new CliError('', 130);
    if (signal) throw new CliError(`${cmd} killed by ${signal}`);
    return { code: code ?? 1, stdout: Buffer.concat(chunks).toString('utf8') };
}

/** Run to completion; any non-zero exit becomes a CliError carrying that code. */
export async function run(env: Env, cmd: string, args: readonly string[], opts: RunOptions = {}): Promise<string> {
    const { code, stdout } = await spawnWait(env, cmd, args, opts);
    if (code !== 0) throw new CliError(`${cmd} exited with code ${code}`, code);
    return stdout;
}

/** Absolute path of an executable on PATH, like `command -v`. */
export function which(cmd: string, path = process.env.PATH ?? ''): string | undefined {
    for (const dir of path.split(delimiter)) {
        if (!dir) continue;
        const full = join(dir, cmd);
        try {
            if (!statSync(full).isFile()) continue;
            accessSync(full, constants.X_OK);
            return full;
        } catch {
            // not here
        }
    }
    return undefined;
}
