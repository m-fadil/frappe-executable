/** A failure meant for the user: printed as-is, no stack trace, exits with `code`. */
export class CliError extends Error {
    code;
    constructor(message, code = 1) {
        super(message);
        this.code = code;
        this.name = 'CliError';
    }
}
/** Bad command line: printed followed by the usage text, exits 2. */
export class UsageError extends CliError {
    constructor(message) {
        super(message, 2);
        this.name = 'UsageError';
    }
}
export function die(message) {
    throw new CliError(message);
}
