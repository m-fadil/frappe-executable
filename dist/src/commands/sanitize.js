import { pickSite, requireBenchDir, siteExists } from '../lib/bench.js';
import { logger } from '../lib/command.js';
import { CliError } from '../lib/errors.js';
import { run } from '../lib/proc.js';
const log = logger('sanitize');
/** table → statement that switches it off; skipped when the table is missing */
const DISABLE = [
    [
        'tabEmail Account',
        'UPDATE `tabEmail Account` SET enable_incoming = 0, enable_outgoing = 0, default_incoming = 0, default_outgoing = 0;',
    ],
    ['tabWebhook', 'UPDATE `tabWebhook` SET enabled = 0;'],
    ['tabNotification', 'UPDATE `tabNotification` SET enabled = 0;'],
    ['tabAuto Repeat', 'UPDATE `tabAuto Repeat` SET status = "Disabled" WHERE status = "Active";'],
    ['tabEmail Queue Recipient', 'DELETE FROM `tabEmail Queue Recipient`;'],
    ['tabEmail Queue', 'DELETE FROM `tabEmail Queue`;'],
];
/** table → [check name, WHERE clause still counting as enabled] */
const VERIFY = [
    ['tabEmail Account', 'email accounts', 'enable_incoming = 1 OR enable_outgoing = 1'],
    ['tabWebhook', 'webhooks', 'enabled = 1'],
    ['tabNotification', 'notifications', 'enabled = 1'],
    ['tabAuto Repeat', 'auto repeats', 'status = "Active"'],
    ['tabEmail Queue', 'queued emails', '1 = 1'],
];
export const sanitize = {
    name: 'sanitize',
    desc: 'Disable outbound integrations on a restored site',
    about: 'Disable outbound integrations on a restored site so it cannot mail or call out.',
    envHelp: `Variables:
  SANITIZE_SITE           asked; falls back to RESTORE_SITE, auto-picked when
                          the bench has one site
  SANITIZE_SINGLES        comma-separated single doctypes to disable
                          default: FCM Settings,TP Twilio Settings,TP Exotel Settings
                          set to empty to skip
  SANITIZE_SITES_DIR      default: sites
  SANITIZE_MUTE_EMAILS    default: 1  site_config mute_emails value`,
    async run(env) {
        if (!env.filled('SANITIZE_SITE') && env.filled('RESTORE_SITE'))
            env.set('SANITIZE_SITE', env.get('RESTORE_SITE'));
        // unset → default list; set but empty → skip the step
        const singles = env.get('SANITIZE_SINGLES') ?? 'FCM Settings,TP Twilio Settings,TP Exotel Settings';
        const sitesDir = env.or('SANITIZE_SITES_DIR', 'sites');
        requireBenchDir(sitesDir);
        const muteEmails = env.or('SANITIZE_MUTE_EMAILS', '1');
        const site = await pickSite(env, 'SANITIZE_SITE', sitesDir);
        const mariadb = (sql, capture = false) => run(env, 'bench', ['--site', site, 'mariadb', ...(capture ? ['-N', '-B'] : []), '-e', sql], { capture });
        log(`Checking site: ${site}`);
        if (!siteExists(sitesDir, site))
            throw new CliError(`[sanitize] Site ${site} does not exist under ${sitesDir}/`);
        log('Muting email queue');
        await run(env, 'bench', ['--site', site, 'set-config', 'mute_emails', muteEmails]);
        const tables = new Set((await mariadb('SHOW TABLES', true))
            .split('\n')
            .map((t) => t.trim())
            .filter(Boolean));
        log('Disabling outbound integrations');
        const statements = ['SET SQL_SAFE_UPDATES = 0;'];
        for (const [table, sql] of DISABLE) {
            if (tables.has(table))
                statements.push(sql);
            else
                log(`skip (table missing): ${table}`);
        }
        await mariadb(statements.join('\n'));
        if (singles === '') {
            log('skip (SANITIZE_SINGLES empty)');
        }
        else if (!tables.has('tabSingles')) {
            log('skip (table missing): tabSingles');
        }
        else {
            // "a , b" -> "a,b" for FIND_IN_SET, and escape quotes
            const list = singles.replace(/ *, */g, ',').replace(/'/g, "''");
            log(`Disabling single doctypes: ${list}`);
            await mariadb(`
                SET SQL_SAFE_UPDATES = 0;

                UPDATE tabSingles
                SET value = '0'
                WHERE field = 'enabled'
                  AND FIND_IN_SET(doctype, '${list}');

                SELECT
                    'singles still enabled' AS check_name,
                    COUNT(*) AS still_enabled
                FROM tabSingles
                WHERE field = 'enabled'
                  AND value = '1';
            `);
        }
        log('Clearing cache');
        await run(env, 'bench', ['--site', site, 'clear-cache']);
        log('Verifying remaining enabled integrations');
        const checks = VERIFY.filter(([table]) => tables.has(table)).map(([table, name, where]) => `    SELECT '${name}' AS check_name, COUNT(*) AS still_enabled FROM \`${table}\` WHERE ${where}`);
        if (checks.length > 0)
            await mariadb(`${checks.join('\n    UNION ALL\n')};`);
        log('Done');
    },
};
