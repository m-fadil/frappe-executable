import { styleText } from 'node:util';
import type { Env } from './env.js';

export const PROG = 'frappe';

export interface Command {
    /** subcommand name: `frappe <name>` */
    name: string;
    /** one line for the command list */
    desc: string;
    /** paragraph under the usage line */
    about: string;
    /** the variables it reads, with defaults; shown by --help */
    envHelp: string;
    run(env: Env): Promise<void>;
}

export function helpText(cmd: Command): string {
    return `Usage: ${PROG} ${cmd.name} [options] [env-file...]

${cmd.about}

Options:
  -h, --help              Show this help
  -e VAR=val, --env       Set/override a variable (repeatable)
      --env-file PATH     Load variables from an env file (repeatable)

${cmd.envHelp}
`;
}

/** `[tag] message` logger, the prefix every site command prints with. */
export function logger(tag: string): (message: string) => void {
    return (message) => console.log(`[${tag}] ${message}`);
}

/** Bold step/section line on stdout; plain when stdout is not a terminal. */
export function heading(text: string, color: 'cyan' | 'yellow' | 'green' = 'cyan'): void {
    console.log(process.stdout.isTTY ? styleText('bold', styleText(color, text)) : text);
}
