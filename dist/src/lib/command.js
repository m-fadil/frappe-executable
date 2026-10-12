import { styleText } from 'node:util';
export const PROG = 'frappe';
export function helpText(cmd) {
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
export function logger(tag) {
    return (message) => console.log(`[${tag}] ${message}`);
}
/** Bold step/section line on stdout; plain when stdout is not a terminal. */
export function heading(text, color = 'cyan') {
    console.log(process.stdout.isTTY ? styleText('bold', styleText(color, text)) : text);
}
