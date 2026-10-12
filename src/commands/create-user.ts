import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pickSite, requireBenchDir, siteExists } from '../lib/bench.js';
import { type Command, PROG } from '../lib/command.js';
import { CliError } from '../lib/errors.js';
import { run } from '../lib/proc.js';
import { askVar } from '../lib/prompt.js';

// values come in via the environment, never interpolated into the source
const SCRIPT = String.raw`import os

import frappe
from frappe.core.doctype.user.user import generate_keys
from frappe.utils.password import get_decrypted_password


def flag(name, default="0"):
    return os.environ.get(name, default).strip().lower() in ("1", "true", "yes", "y")


email = os.environ["USER_EMAIL"]
roles = [r.strip() for r in os.environ.get("USER_ROLES", "").split(",") if r.strip()]
password = os.environ.get("USER_PASSWORD", "")
welcome = flag("USER_WELCOME_EMAIL")

for role in roles:
    if not frappe.db.exists("Role", role):
        raise SystemExit(f"Role does not exist on this site: {role}")

if frappe.db.exists("User", email):
    user = frappe.get_doc("User", email)
    print(f"User already exists, left unchanged: {email}")
else:
    user = frappe.get_doc({
        "doctype": "User",
        "email": email,
        "first_name": os.environ["USER_FIRST_NAME"],
        "last_name": os.environ.get("USER_LAST_NAME", ""),
        "enabled": 1,
        "send_welcome_email": 1 if welcome else 0,
        "roles": [{"role": r} for r in roles],
    })
    user.flags.no_welcome_mail = not welcome
    if password:
        user.new_password = password
    user.insert(ignore_permissions=True)
    print(f"User created: {email}")
    if roles:
        print(f"Roles: {', '.join(roles)}")

if flag("USER_API_KEY", "1"):
    if user.api_key:
        api_secret = get_decrypted_password("User", user.name, "api_secret", raise_exception=True)
    else:
        api_secret = generate_keys(user.name)["api_secret"]
        user.reload()
        print("API credentials generated")
    print()
    print(f"{user.api_key}:{api_secret}")

frappe.db.commit()
`;

export const createUser: Command = {
    name: 'create-user',
    desc: 'Create a user on a site, optionally print API credentials',
    about: 'Create (or reuse) a user on a site in the current bench.',
    envHelp: `Variables:
  USER_SITE               site name; single site is auto-picked, otherwise asked
  USER_EMAIL              required; asked when unset
  USER_FIRST_NAME         asked when unset, defaults to the email local part
  USER_LAST_NAME          optional
  USER_ROLES              comma-separated roles, default: System Manager
                          set to empty for no extra roles
  USER_PASSWORD           optional; set only when non-empty
  USER_API_KEY            default: 1  generate/print api_key:api_secret
  USER_WELCOME_EMAIL      default: 0  send the welcome email
  USER_SITES_DIR          default: sites

Examples:
  ${PROG} create-user -e USER_EMAIL=mcp@example.com
  ${PROG} create-user -e USER_EMAIL=ops@example.com -e USER_FIRST_NAME=Ops \\
      -e USER_ROLES='Accounts User,Sales User' -e USER_API_KEY=0
  ${PROG} create-user --env-file user.env`,

    async run(env) {
        const sitesDir = env.or('USER_SITES_DIR', 'sites');
        requireBenchDir(sitesDir);

        const site = await pickSite(env, 'USER_SITE', sitesDir);
        if (!siteExists(sitesDir, site)) throw new CliError(`Site ${site} does not exist under ${sitesDir}/`);

        const email = await askVar(env, 'USER_EMAIL', 'User email');
        if (!/@.*\./.test(email)) throw new CliError(`not an email address: ${email}`);
        const firstName = await askVar(env, 'USER_FIRST_NAME', 'First name', email.split('@')[0]!);

        const vars = {
            USER_EMAIL: email,
            USER_FIRST_NAME: firstName,
            USER_LAST_NAME: env.or('USER_LAST_NAME', ''),
            // unset → System Manager; set but empty → no roles
            USER_ROLES: env.get('USER_ROLES') ?? 'System Manager',
            USER_PASSWORD: env.or('USER_PASSWORD', ''),
            USER_API_KEY: env.or('USER_API_KEY', '1'),
            USER_WELCOME_EMAIL: env.or('USER_WELCOME_EMAIL', '0'),
        };

        console.log(`Site       : ${site}`);
        console.log(`Email      : ${email}`);
        console.log(`Name       : ${[firstName, vars.USER_LAST_NAME].filter(Boolean).join(' ')}`);
        console.log(`Roles      : ${vars.USER_ROLES || '<none>'}`);
        console.log(`API key    : ${vars.USER_API_KEY === '0' ? 'no' : 'yes'}`);
        console.log();

        const dir = mkdtempSync(join(tmpdir(), 'frappe-create-user-'));
        const script = join(dir, 'create_user.py');
        try {
            writeFileSync(script, SCRIPT, { mode: 0o600 });
            const path = JSON.stringify(script); // a JSON string is a valid Python string literal
            await run(
                env,
                'bench',
                ['--site', site, 'execute', `exec(compile(open(${path}).read(), ${path}, 'exec'))`],
                {
                    extraEnv: vars,
                },
            );
        } finally {
            rmSync(dir, { recursive: true, force: true });
        }
    },
};
