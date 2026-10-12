import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { loadEnv, parseArgs } from '../src/lib/env.js';
import { parseEnvFile } from '../src/lib/envfile.js';
import { CliError, UsageError } from '../src/lib/errors.js';

const dir = mkdtempSync(join(tmpdir(), 'fe-test-'));
after(() => rmSync(dir, { recursive: true, force: true }));
const file = (name: string, text: string) => {
    const p = join(dir, name);
    writeFileSync(p, text);
    return p;
};
const one = file('one.env', 'A=from_file\nB=b_file\n');
const two = file('two.env', 'A=second_file\n');

const load = (...argv: string[]) => loadEnv(parseArgs(argv), {});

describe('argument parsing and precedence', () => {
    test('bare positional, --env-file and --env-file= all load a file', () => {
        for (const argv of [[one], ['--env-file', one], [`--env-file=${one}`]]) {
            const env = load(...argv);
            assert.equal(env.get('A'), 'from_file');
            assert.equal(env.get('B'), 'b_file');
        }
    });

    test('later env file wins', () => {
        assert.equal(load(one, two).get('A'), 'second_file');
    });

    test('-e beats every file regardless of order', () => {
        const env = load('-e', 'A=cli', one, two);
        assert.equal(env.get('A'), 'cli');
        assert.equal(env.get('B'), 'b_file');
    });

    test('file beats the process environment', () => {
        assert.equal(loadEnv(parseArgs([one]), { A: 'process', Z: 'z' }).get('A'), 'from_file');
        assert.equal(loadEnv(parseArgs([one]), { A: 'process', Z: 'z' }).get('Z'), 'z');
    });

    test('all override spellings, repeatable, value keeps = and spaces', () => {
        const env = load('-eA=1', '--env', 'B=2', '--env=C=a=b c', '-e', 'D=');
        assert.deepEqual(
            ['A', 'B', 'C', 'D'].map((k) => env.get(k)),
            ['1', '2', 'a=b c', ''],
        );
    });

    test('-- turns everything after it into env files', () => {
        assert.deepEqual(parseArgs(['--', '-weird.env']).envFiles, ['-weird.env']);
    });

    test('-h stops parsing, even after a bad option would have failed', () => {
        assert.equal(parseArgs(['-h', '--nope']).help, true);
        assert.equal(parseArgs(['--help']).help, true);
    });

    test('usage errors', () => {
        assert.throws(
            () => parseArgs(['--nope']),
            (e) => e instanceof UsageError && /unknown option: --nope/.test(e.message),
        );
        assert.throws(() => parseArgs(['-']), /unknown option: -$/);
        assert.throws(() => parseArgs(['-e', 'notanassignment']), /invalid override: notanassignment/);
        assert.throws(() => parseArgs(['-e', '1A=x']), /invalid override/);
        assert.throws(() => parseArgs(['-e']), /requires VAR=val/);
        assert.throws(() => parseArgs(['--env-file']), /requires a path/);
    });

    test('missing env file', () => {
        assert.throws(
            () => load(join(dir, 'absent.env')),
            (e) => e instanceof CliError && /env file not found/.test(e.message),
        );
    });

    test('unset vs empty: or() falls back on both, get() tells them apart', () => {
        const env = load('-e', 'EMPTY=');
        assert.equal(env.get('EMPTY'), '');
        assert.equal(env.get('UNSET'), undefined);
        assert.equal(env.or('EMPTY', 'd'), 'd');
        assert.equal(env.or('UNSET', 'd'), 'd');
    });
});

describe('env file syntax', () => {
    const parse = (text: string, vars: Record<string, string> = {}) =>
        Object.fromEntries(parseEnvFile(text, 'x.env', (n) => vars[n]));

    test('comments, blank lines, export prefix, trailing comments', () => {
        assert.deepEqual(parse('# c\n\nexport A=1\nB=2   # note\nC=   # empty\nD=a#b\n'), {
            A: '1',
            B: '2',
            C: '',
            D: 'a#b',
        });
    });

    test('quoting', () => {
        assert.deepEqual(parse(`A='lit $X "q"'\nB="dq \\" \\\\ \\$X"\nC="a b"'c d'\nD=""`, { X: 'x' }), {
            A: 'lit $X "q"',
            B: 'dq " \\ $X',
            C: 'a bc d',
            D: '',
        });
    });

    test('expansion from earlier lines, then the lookup; unknown is empty', () => {
        assert.deepEqual(parse('A=1\nB=$A-${A}\nC="$OUTER/$NOPE"\nD=$\n', { OUTER: 'o', A: 'ignored' }), {
            A: '1',
            B: '1-1',
            C: 'o/',
            D: '$',
        });
    });

    test('CRLF line endings', () => {
        assert.deepEqual(parse('A=1\r\nB="2"\r\n'), { A: '1', B: '2' });
    });

    test('quoted values may span lines; line numbers stay right after them', () => {
        assert.deepEqual(parse('A="one\ntwo"\nB=\'x\n# not a comment\'\nC=3\n'), {
            A: 'one\ntwo',
            B: 'x\n# not a comment',
            C: '3',
        });
        assert.throws(() => parse('A="one\ntwo"\nB=bad value\n'), /x\.env:3: unquoted whitespace/);
    });

    test('a UTF-8 BOM and bare `export NAME` lines are accepted', () => {
        assert.deepEqual(parse('\uFEFFA=1\nexport A\nexport B # marks only\n'), { A: '1' });
    });

    test('errors name file and line', () => {
        assert.throws(() => parse('A=1\nB=two words\n'), /x\.env:2: unquoted whitespace/);
        assert.throws(() => parse("A='open\n"), /x\.env:1: unterminated single quote/);
        assert.throws(() => parse('A="open\n'), /unterminated double quote/);
        assert.throws(() => parse('not an assignment\n'), /x\.env:1: expected KEY=value/);
        assert.throws(() => parse('A=${B:-x}\n'), /unsupported expansion/);
    });

    test('.env.example parses with nothing set', () => {
        const example = fileURLToPath(new URL('../../.env.example', import.meta.url));
        const env = loadEnv(parseArgs([example]), {});
        assert.equal(env.get('MODE'), 'docker');
        assert.equal(env.get('DB_ROOT_PASS'), '');
        assert.equal(env.get('RESTORE_SOURCE_TOKEN'), 'token abcdef123456:secret789');
    });
});
