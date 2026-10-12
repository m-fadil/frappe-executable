#!/usr/bin/env node
import { existsSync, readFileSync, realpathSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { benchInit } from './commands/bench-init.js';
import { createUser } from './commands/create-user.js';
import { restore } from './commands/restore.js';
import { restoreSanitize } from './commands/restore-sanitize.js';
import { sanitize } from './commands/sanitize.js';
import { type Command, helpText, PROG } from './lib/command.js';
import { Env, loadEnv, parseArgs } from './lib/env.js';
import { CliError, UsageError } from './lib/errors.js';
import { spawnWait } from './lib/proc.js';

const COMMANDS: readonly Command[] = [benchInit, restore, sanitize, restoreSanitize, createUser];

/** package root: <root>/dist/src/cli.js */
const ROOT = join(dirname(realpathSync(fileURLToPath(import.meta.url))), '..', '..');
const PKG = JSON.parse(readFileSync(join(ROOT, 'package.json'), 'utf8')) as { name: string; version: string };

function usage(): string {
    const row = (name: string, desc: string) => `  ${name.padEnd(18)} ${desc}`;
    return [
        `Usage: ${PROG} <command> [options] [env-file...]`,
        '',
        'Commands:',
        ...COMMANDS.map((c) => row(c.name, c.desc)),
        '',
        'Self:',
        row('uninstall', 'Remove the global npm install'),
        row('--version', 'Show the installed version'),
        '',
        'Every command accepts -h/--help, -e VAR=val and --env-file PATH.',
        `Run "${PROG} <command> --help" for its variables.`,
        '',
        'Commands act on the bench in the CURRENT directory.',
        '',
    ].join('\n');
}

/**
 * `npm uninstall -g` against the prefix we actually live in, which may not be
 * the current npm's (nvm, a custom --prefix). A checkout is never removed.
 */
async function uninstall(): Promise<number> {
    if (existsSync(join(ROOT, '.git'))) {
        throw new CliError(`${ROOT} is a git checkout, not an install — delete it with git/rm instead`);
    }
    // global installs live at <prefix>/lib/node_modules/<pkg> (Linux/macOS)
    const modules = dirname(ROOT);
    const lib = dirname(modules);
    if (basename(ROOT) !== PKG.name || basename(modules) !== 'node_modules' || basename(lib) !== 'lib') {
        throw new CliError(`${ROOT} is not a global npm install — remove it by hand`);
    }
    const prefix = dirname(lib);

    const npm = ['uninstall', '-g', '--prefix', prefix, PKG.name];
    const { code } = await spawnWait(new Env(process.env), 'npm', npm);
    if (code !== 0) {
        console.error(
            `npm uninstall failed; if ${prefix} needs root: sudo npm uninstall -g --prefix ${prefix} ${PKG.name}`,
        );
        return code;
    }
    console.log(`uninstalled ${PKG.name} from ${prefix}`);
    return 0;
}

async function main(argv: string[]): Promise<number> {
    const [name, ...rest] = argv;
    switch (name) {
        case undefined:
        case '-h':
        case '--help':
        case 'help':
            process.stdout.write(usage());
            return 0;
        case '--version':
        case '-V':
            console.log(PKG.version);
            return 0;
        case 'uninstall':
            return uninstall();
    }

    const cmd = COMMANDS.find((c) => c.name === name);
    if (!cmd) {
        process.stderr.write(`unknown command: ${name}\n\n${usage()}`);
        return 2;
    }

    let args;
    try {
        args = parseArgs(rest);
    } catch (err) {
        if (!(err instanceof UsageError)) throw err;
        process.stderr.write(`${err.message}\n\n${helpText(cmd)}`);
        return 2;
    }
    if (args.help) {
        process.stdout.write(helpText(cmd));
        return 0;
    }
    await cmd.run(loadEnv(args));
    return 0;
}

try {
    process.exitCode = await main(process.argv.slice(2));
} catch (err) {
    if (err instanceof CliError) {
        if (err.message) console.error(err.message);
        process.exitCode = err.code;
    } else if (err instanceof Error && err.name === 'ExitPromptError') {
        // Ctrl+C inside a prompt
        process.exitCode = 130;
    } else {
        throw err;
    }
}
