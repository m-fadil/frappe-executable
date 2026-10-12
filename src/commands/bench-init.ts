import { type Command, heading } from '../lib/command.js';
import type { Env } from '../lib/env.js';
import { CliError } from '../lib/errors.js';
import { run } from '../lib/proc.js';
import {
    askSelect,
    canPrompt,
    promptConfirm,
    promptSecret,
    promptSelect,
    promptText,
    Separator,
} from '../lib/prompt.js';

type Mode = 'server' | 'docker';

interface Field {
    key: string;
    label: string;
    secret?: boolean;
    required?: boolean;
}

const COMMON_FIELDS: Field[] = [
    { key: 'FRAPPE_VERSION', label: 'Frappe Version' },
    { key: 'BENCH_NAME', label: 'Bench Name' },
    { key: 'SITE_NAME', label: 'Site Name' },
    { key: 'DB_HOST', label: 'DB Host' },
    { key: 'DB_PORT', label: 'DB Port' },
    { key: 'DB_NAME', label: 'DB Name' },
    { key: 'DB_ROOT_USER', label: 'DB Root User' },
    { key: 'DB_ROOT_PASS', label: 'DB Root Password', secret: true, required: true },
    { key: 'ADMIN_PASS', label: 'Admin Password', secret: true, required: true },
];

const DOCKER_FIELDS: Field[] = [
    { key: 'REDIS_CACHE', label: 'Redis Cache' },
    { key: 'REDIS_QUEUE', label: 'Redis Queue' },
    { key: 'REDIS_SOCKETIO', label: 'Redis SocketIO' },
];

/** Default DB name for a site: every non-alphanumeric becomes `_`. */
const dbNameFor = (site: string) => site.replace(/[^a-zA-Z0-9]/g, '_');

/** Fill every unset field with its mode-specific default. */
function applyDefaults(env: Env, mode: Mode): void {
    const site = env.or('SITE_NAME', 'development.localhost');
    const defaults: Record<string, string> = {
        FRAPPE_VERSION: 'version-16',
        BENCH_NAME: 'frappe-bench',
        SITE_NAME: site,
        DB_HOST: mode === 'docker' ? 'mariadb' : '127.0.0.1',
        DB_PORT: '3306',
        DB_NAME: dbNameFor(site),
        DB_ROOT_USER: 'root',
        ...(mode === 'docker' && {
            REDIS_CACHE: 'redis://redis-cache:6379',
            REDIS_QUEUE: 'redis://redis-queue:6379',
            REDIS_SOCKETIO: 'redis://redis-socketio:6379',
        }),
    };
    for (const [key, value] of Object.entries(defaults)) env.set(key, env.or(key, value));
}

function display(env: Env, field: Field): string {
    if (!env.filled(field.key)) return '<not set>';
    return field.secret ? '••••••••' : env.get(field.key)!;
}

function missingRequired(env: Env, fields: Field[]): string[] {
    return fields.filter((f) => f.required && !env.filled(f.key)).map((f) => f.key);
}

async function editField(env: Env, field: Field): Promise<void> {
    if (field.secret) {
        // an empty answer keeps a secret that is already set
        const keep = env.filled(field.key);
        const value = await promptSecret(keep ? `${field.label} (empty keeps current)` : field.label, keep);
        if (value !== '') env.set(field.key, value);
        return;
    }
    env.set(field.key, await promptText(field.label, { default: env.get(field.key), required: true }));
}

const APPLY = '__apply__';
const QUIT = '__quit__';

/** The config menu: pick a field to edit, or apply / quit. Resolves true to run. */
async function configure(env: Env, mode: Mode, fields: Field[]): Promise<boolean> {
    for (const field of fields) if (field.required && !env.filled(field.key)) await editField(env, field);

    const width = Math.max(...fields.map((f) => f.label.length)) + 2;
    let cursor: string = APPLY;
    for (;;) {
        cursor = await promptSelect(
            `Frappe bench init (mode: ${mode}) — pick a value to change`,
            [
                ...fields.map((f) => ({ value: f.key, name: `${f.label.padEnd(width)}${display(env, f)}` })),
                new Separator(),
                { value: APPLY, name: 'Apply & run' },
                { value: QUIT, name: 'Quit' },
            ],
            cursor,
        );
        if (cursor === QUIT) return false;
        if (cursor !== APPLY) {
            // DB Name tracks Site Name until it is given a value of its own
            const follows = cursor === 'SITE_NAME' && env.get('DB_NAME') === dbNameFor(env.get('SITE_NAME')!);
            await editField(
                env,
                fields.find((f) => f.key === cursor)!,
            );
            if (follows) env.set('DB_NAME', dbNameFor(env.get('SITE_NAME')!));
            continue;
        }
        const missing = missingRequired(env, fields);
        if (missing.length > 0) {
            console.error(`Still missing: ${missing.join(', ')}`);
            continue;
        }
        if (await promptConfirm('Run bench init now?')) return true;
    }
}

async function runBench(env: Env, mode: Mode): Promise<void> {
    const v = (k: string) => env.need(k);
    const words = (k: string) => (env.get(k) ?? '').split(/\s+/).filter(Boolean);

    const initArgs = ['init', '--ignore-exist', `--frappe-branch=${v('FRAPPE_VERSION')}`, '--dev'];
    const siteArgs = [
        'new-site',
        `--db-name=${v('DB_NAME')}`,
        `--db-root-username=${v('DB_ROOT_USER')}`,
        `--db-root-password=${v('DB_ROOT_PASS')}`,
        `--admin-password=${v('ADMIN_PASS')}`,
    ];
    if (mode === 'docker') {
        initArgs.push('--skip-redis-config-generation');
        siteArgs.push('--mariadb-user-host-login-scope=%');
    }
    initArgs.push(...words('BENCH_INIT_EXTRA'), v('BENCH_NAME'));
    siteArgs.push(...words('BENCH_NEW_SITE_EXTRA'), v('SITE_NAME'));

    heading(`\n== Running bench init (mode: ${mode}) ==\n`);

    heading('[1/3] Initialising bench...', 'yellow');
    await run(env, 'bench', initArgs);

    const cwd = v('BENCH_NAME');
    heading('\n[2/3] Setting global config...', 'yellow');
    await run(env, 'bench', ['set-config', '-g', 'db_host', v('DB_HOST')], { cwd });
    await run(env, 'bench', ['set-config', '-g', 'db_port', v('DB_PORT')], { cwd });
    if (mode === 'docker') {
        for (const key of ['redis_cache', 'redis_queue', 'redis_socketio']) {
            await run(env, 'bench', ['set-config', '-g', key, v(key.toUpperCase())], { cwd });
        }
    } else {
        console.log('  (server mode: bench generates its own redis config)');
    }

    heading('\n[3/3] Creating site...', 'yellow');
    await run(env, 'bench', siteArgs, { cwd });

    heading('\n✔ Setup complete\n', 'green');
}

export const benchInit: Command = {
    name: 'bench-init',
    desc: 'Init a new bench + site (server or docker layout)',
    about: 'Interactive Frappe bench init: pick a layout, review every value, then run.',
    envHelp: `Variables:
  MODE                    server | docker (asked when unset)
  YES                     non-empty: skip the menu and run immediately
  FRAPPE_VERSION          default: version-16
  BENCH_NAME              default: frappe-bench
  SITE_NAME               default: development.localhost
  DB_HOST                 default: mariadb (docker) / 127.0.0.1 (server)
  DB_PORT                 default: 3306
  DB_NAME                 default: derived from SITE_NAME
  DB_ROOT_USER            default: root
  DB_ROOT_PASS            required (no default)
  ADMIN_PASS              required (no default)
  REDIS_CACHE             docker only, default: redis://redis-cache:6379
  REDIS_QUEUE             docker only, default: redis://redis-queue:6379
  REDIS_SOCKETIO          docker only, default: redis://redis-socketio:6379
  BENCH_INIT_EXTRA        extra args appended to 'bench init'
  BENCH_NEW_SITE_EXTRA    extra args appended to 'bench new-site'

Docker mode adds --skip-redis-config-generation to 'bench init', writes the
redis_* global config, and creates the site with
--mariadb-user-host-login-scope=%. Server mode does none of those.`,

    async run(env) {
        const unattended = env.filled('YES');
        const interactive = !unattended && canPrompt();

        if (!env.filled('MODE') && !interactive) {
            throw new CliError('MODE is required when running non-interactively (-e MODE=server|docker)');
        }
        const mode = await askSelect(env, 'MODE', 'Target deployment', [
            { value: 'server', description: 'bench manages redis itself, DB on 127.0.0.1' },
            { value: 'docker', description: 'redis & mariadb as separate services (devcontainer)' },
        ]);
        if (mode !== 'server' && mode !== 'docker') {
            throw new CliError(`MODE must be 'server' or 'docker' (got: ${mode})`);
        }

        applyDefaults(env, mode);
        const fields = mode === 'docker' ? [...COMMON_FIELDS, ...DOCKER_FIELDS] : COMMON_FIELDS;

        if (unattended) {
            const missing = missingRequired(env, fields);
            if (missing.length > 0) throw new CliError(`missing required: ${missing.join(' ')}`);
        } else {
            if (!interactive) throw new CliError('no tty: set YES=1 to run non-interactively');
            if (!(await configure(env, mode, fields))) {
                console.log('Cancelled.');
                return;
            }
        }
        await runBench(env, mode);
    },
};
