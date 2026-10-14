import { confirm, input, password, search, select, Separator } from '@inquirer/prompts';
import { CliError } from './errors.js';
/**
 * Prompts are drawn on stderr, like `read -p` would, so stdout stays clean for
 * piping (`frappe create-user ... | tail -1`). Ctrl+C surfaces as inquirer's
 * ExitPromptError, which the CLI turns into exit 130.
 */
const CONTEXT = { input: process.stdin, output: process.stderr, clearPromptOnDone: false };
export const canPrompt = () => Boolean(process.stdin.isTTY && process.stderr.isTTY);
function noTerminal(name, extra = '') {
    return new CliError(`${name} is required (${extra}no terminal to ask on; pass -e ${name}=... or --env-file)`);
}
export function promptText(message, opts = {}) {
    return input({ message, default: opts.default || undefined, required: opts.required ?? false }, CONTEXT).then((v) => v.trim());
}
export function promptSecret(message, allowEmpty = false) {
    return password({ message, mask: '•', validate: (v) => allowEmpty || v.length > 0 || 'Value is required' }, CONTEXT);
}
export function promptSelect(message, choices, defaultValue) {
    return select({ message, choices, default: defaultValue, pageSize: 20, loop: false }, CONTEXT);
}
export function promptConfirm(message, defaultValue = true) {
    return confirm({ message, default: defaultValue }, CONTEXT);
}
/** Type-to-filter list; Tab copies the highlighted choice's name into the input. */
export function promptSearch(message, source, validate) {
    return search({ message, source: (term) => source(term ?? ''), validate, pageSize: 15 }, CONTEXT);
}
export { Separator };
// --- "ask only when unset" fallbacks: -e / --env-file always win -----------
/** Ask for `name` unless set. Without a terminal, use `fallback` or fail naming the variable. */
export async function askVar(env, name, message, fallback = '') {
    if (env.filled(name))
        return env.get(name);
    let value = fallback;
    if (canPrompt())
        value = await promptText(message, { default: fallback, required: fallback === '' });
    else if (fallback === '')
        throw noTerminal(name);
    if (value === '')
        throw new CliError(`${name} is required`);
    env.set(name, value);
    return value;
}
/** Ask for a secret unless set: masked, never defaulted. */
export async function askSecret(env, name, message) {
    if (env.filled(name))
        return env.get(name);
    if (!canPrompt())
        throw noTerminal(name);
    const value = await promptSecret(message);
    env.set(name, value);
    return value;
}
/** Arrow-key pick unless set. */
export async function askSelect(env, name, message, choices, extra = '') {
    if (env.filled(name))
        return env.get(name);
    if (!canPrompt())
        throw noTerminal(name, extra);
    const value = await promptSelect(message, choices);
    env.set(name, value);
    return value;
}
