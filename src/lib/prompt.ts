import { confirm, input, password, select, Separator } from '@inquirer/prompts';
import type { Env } from './env.js';
import { CliError } from './errors.js';

/**
 * Prompts are drawn on stderr, like `read -p` would, so stdout stays clean for
 * piping (`frappe create-user ... | tail -1`). Ctrl+C surfaces as inquirer's
 * ExitPromptError, which the CLI turns into exit 130.
 */
const CONTEXT = { input: process.stdin, output: process.stderr, clearPromptOnDone: false };

export const canPrompt = (): boolean => Boolean(process.stdin.isTTY && process.stderr.isTTY);

function noTerminal(name: string, extra = ''): CliError {
    return new CliError(`${name} is required (${extra}no terminal to ask on; pass -e ${name}=... or --env-file)`);
}

export function promptText(message: string, opts: { default?: string; required?: boolean } = {}): Promise<string> {
    return input({ message, default: opts.default || undefined, required: opts.required ?? false }, CONTEXT).then((v) =>
        v.trim(),
    );
}

export function promptSecret(message: string, allowEmpty = false): Promise<string> {
    return password(
        { message, mask: '•', validate: (v) => allowEmpty || v.length > 0 || 'Value is required' },
        CONTEXT,
    );
}

export interface Choice<T extends string> {
    value: T;
    name?: string;
    description?: string;
}

export function promptSelect<T extends string>(
    message: string,
    choices: ReadonlyArray<Choice<T> | Separator>,
    defaultValue?: T,
): Promise<T> {
    return select<T>({ message, choices, default: defaultValue, pageSize: 20, loop: false }, CONTEXT);
}

export function promptConfirm(message: string, defaultValue = true): Promise<boolean> {
    return confirm({ message, default: defaultValue }, CONTEXT);
}

export { Separator };

// --- "ask only when unset" fallbacks: -e / --env-file always win -----------

/** Ask for `name` unless set. Without a terminal, use `fallback` or fail naming the variable. */
export async function askVar(env: Env, name: string, message: string, fallback = ''): Promise<string> {
    if (env.filled(name)) return env.get(name)!;
    let value = fallback;
    if (canPrompt()) value = await promptText(message, { default: fallback, required: fallback === '' });
    else if (fallback === '') throw noTerminal(name);
    if (value === '') throw new CliError(`${name} is required`);
    env.set(name, value);
    return value;
}

/** Ask for a secret unless set: masked, never defaulted. */
export async function askSecret(env: Env, name: string, message: string): Promise<string> {
    if (env.filled(name)) return env.get(name)!;
    if (!canPrompt()) throw noTerminal(name);
    const value = await promptSecret(message);
    env.set(name, value);
    return value;
}

/** Arrow-key pick unless set. */
export async function askSelect<T extends string>(
    env: Env,
    name: string,
    message: string,
    choices: ReadonlyArray<Choice<T>>,
    extra = '',
): Promise<string> {
    if (env.filled(name)) return env.get(name)!;
    if (!canPrompt()) throw noTerminal(name, extra);
    const value = await promptSelect(message, choices);
    env.set(name, value);
    return value;
}
