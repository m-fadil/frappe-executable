import { existsSync, readFileSync, statSync } from 'node:fs';
import { parseEnvFile } from './envfile.js';
import { CliError, UsageError } from './errors.js';
/**
 * The variables a command runs with: the process environment, then each env
 * file in order (later wins), then every `-e` override (always wins).
 * Child processes get the merged result, just as exported shell vars would.
 */
export class Env {
    vars;
    constructor(base = {}) {
        this.vars = new Map();
        for (const [k, v] of Object.entries(base))
            if (v !== undefined)
                this.vars.set(k, v);
    }
    /** Raw value; `undefined` only when unset (an empty string is a value). */
    get(name) {
        return this.vars.get(name);
    }
    /** Value, or `fallback` when unset OR empty (shell `${NAME:-fallback}`). */
    or(name, fallback) {
        const v = this.vars.get(name);
        return v === undefined || v === '' ? fallback : v;
    }
    /** True when set to a non-empty value. */
    filled(name) {
        return (this.vars.get(name) ?? '') !== '';
    }
    /** Non-empty value or a CliError naming the variable. */
    need(name) {
        const v = this.vars.get(name);
        if (v === undefined || v === '')
            throw new CliError(`${name} is required`);
        return v;
    }
    set(name, value) {
        this.vars.set(name, value);
    }
    toProcessEnv(extra = {}) {
        return { ...Object.fromEntries(this.vars), ...extra };
    }
}
/**
 * Options every command accepts:
 *   -h | --help
 *   -e VAR=val | -eVAR=val | --env VAR=val | --env=VAR=val   (repeatable)
 *   --env-file PATH | --env-file=PATH                        (repeatable)
 *   PATH                                                     (bare positional = env file)
 *   --                                                       (everything after is an env file)
 * Stops at the first -h/--help, so `-h` works even next to a bad option.
 */
export function parseArgs(argv) {
    const res = { help: false, envFiles: [], overrides: [] };
    const override = (kv) => {
        const m = /^([A-Za-z_][A-Za-z0-9_]*)=([\s\S]*)$/.exec(kv);
        if (!m)
            throw new UsageError(`invalid override: ${kv} (expected VAR=val)`);
        res.overrides.push([m[1], m[2]]);
    };
    for (let i = 0; i < argv.length; i++) {
        const a = argv[i];
        const next = (what) => {
            if (i + 1 >= argv.length)
                throw new UsageError(`${a} requires ${what}`);
            return argv[++i];
        };
        if (a === '-h' || a === '--help') {
            res.help = true;
            return res;
        }
        else if (a === '-e' || a === '--env')
            override(next('VAR=val'));
        else if (a.startsWith('--env='))
            override(a.slice('--env='.length));
        else if (a.startsWith('-e'))
            override(a.slice(2));
        else if (a === '--env-file')
            res.envFiles.push(next('a path'));
        else if (a.startsWith('--env-file='))
            res.envFiles.push(a.slice('--env-file='.length));
        else if (a === '--')
            (res.envFiles.push(...argv.slice(i + 1)), (i = argv.length));
        else if (a.startsWith('-'))
            throw new UsageError(`unknown option: ${a}`);
        else
            res.envFiles.push(a);
    }
    return res;
}
export function loadEnv(args, base = process.env) {
    const env = new Env(base);
    for (const file of args.envFiles) {
        if (!existsSync(file) || !statSync(file).isFile())
            throw new CliError(`env file not found: ${file}`);
        let text;
        try {
            text = readFileSync(file, 'utf8');
        }
        catch (err) {
            throw new CliError(`cannot read env file: ${file}: ${err.message}`);
        }
        for (const [k, v] of parseEnvFile(text, file, (n) => env.get(n)))
            env.set(k, v);
    }
    for (const [k, v] of args.overrides)
        env.set(k, v);
    return env;
}
