import { CliError } from './errors.js';
/**
 * Parse a dotenv-style file with the shell rules people actually write:
 *
 *   # comment                    KEY=value          export KEY=value
 *   KEY='literal $not expanded'  KEY="a $B ${C} \" \\ \$"
 *   KEY=value   # trailing comment (needs whitespace before '#')
 *
 * `$NAME` / `${NAME}` expand from `lookup` (earlier lines, earlier files, the
 * process environment). An unquoted value containing whitespace is an error
 * rather than being silently cut, since the shell would run it as a command.
 */
export function parseEnvFile(text, file, lookup) {
    const out = [];
    const seen = new Map();
    const get = (name) => (seen.has(name) ? seen.get(name) : lookup(name));
    const lines = text.replace(/^\uFEFF/, '').split(/\r?\n/);
    for (let idx = 0; idx < lines.length; idx++) {
        const where = `${file}:${idx + 1}`;
        let line = lines[idx].trimStart();
        if (line === '' || line.startsWith('#'))
            continue;
        const exported = /^export\s+/.test(line);
        line = line.replace(/^export\s+/, '');
        // `export NAME` alone only marks a variable exported: nothing to load
        if (exported && /^[A-Za-z_][A-Za-z0-9_]*\s*(#.*)?$/.test(line))
            continue;
        const m = /^([A-Za-z_][A-Za-z0-9_]*)=/.exec(line);
        if (!m)
            throw new CliError(`${where}: expected KEY=value, got: ${lines[idx].trim()}`);
        const key = m[1];
        let src = line.slice(m[0].length);
        // a quoted value may span lines, as it would when sourced by a shell
        let value;
        for (;;) {
            try {
                value = parseValue(src, where, get);
                break;
            }
            catch (err) {
                if (!(err instanceof UnterminatedQuote) || idx + 1 >= lines.length)
                    throw err;
                src += `\n${lines[++idx]}`;
            }
        }
        seen.set(key, value);
        out.push([key, value]);
    }
    return out;
}
const NAME = /^[A-Za-z_][A-Za-z0-9_]*/;
/** Thrown mid-value so the caller can append the next line and try again. */
class UnterminatedQuote extends CliError {
}
function parseValue(src, where, get) {
    let out = '';
    let i = 0;
    // $NAME or ${NAME} starting at src[i] === '$'; returns [expansion, chars consumed]
    const expand = () => {
        const rest = src.slice(i + 1);
        if (rest.startsWith('{')) {
            const end = rest.indexOf('}');
            const name = end === -1 ? '' : rest.slice(1, end);
            if (!NAME.test(name) || NAME.exec(name)[0] !== name) {
                throw new CliError(`${where}: unsupported expansion \${${end === -1 ? rest.slice(1) : name}}`);
            }
            return [get(name) ?? '', end + 2];
        }
        const m = NAME.exec(rest);
        if (!m)
            return ['$', 1];
        return [get(m[0]) ?? '', m[0].length + 1];
    };
    while (i < src.length) {
        const c = src[i];
        if (c === "'") {
            const end = src.indexOf("'", i + 1);
            if (end === -1)
                throw new UnterminatedQuote(`${where}: unterminated single quote`);
            out += src.slice(i + 1, end);
            i = end + 1;
        }
        else if (c === '"') {
            i++;
            for (;;) {
                if (i >= src.length)
                    throw new UnterminatedQuote(`${where}: unterminated double quote`);
                const d = src[i];
                if (d === '"') {
                    i++;
                    break;
                }
                if (d === '\\' && i + 1 < src.length && '"\\$`'.includes(src[i + 1])) {
                    out += src[i + 1];
                    i += 2;
                }
                else if (d === '$') {
                    const [v, n] = expand();
                    out += v;
                    i += n;
                }
                else {
                    out += d;
                    i++;
                }
            }
        }
        else if (c === ' ' || c === '\t') {
            const rest = src.slice(i).trimStart();
            if (rest !== '' && !rest.startsWith('#')) {
                throw new CliError(`${where}: unquoted whitespace in value — wrap the value in quotes`);
            }
            break;
        }
        else if (c === '\\' && i + 1 < src.length) {
            out += src[i + 1];
            i += 2;
        }
        else if (c === '$') {
            const [v, n] = expand();
            out += v;
            i += n;
        }
        else {
            out += c;
            i++;
        }
    }
    return out;
}
