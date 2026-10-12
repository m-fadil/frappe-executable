import { pickSite, requireBenchDir } from '../lib/bench.js';
import { restore } from './restore.js';
import { sanitize } from './sanitize.js';
const banner = (title) => console.log(`========================================\n ${title}\n========================================`);
export const restoreSanitize = {
    name: 'restore-sanitize',
    desc: 'Run restore then sanitize in one go',
    about: 'Run restore then sanitize with the same options.',
    envHelp: `No env file is required: anything missing is asked for on the terminal.
Both steps read the same variables, so see 'frappe restore --help' and
'frappe sanitize --help'. The site is asked for once and used by both.`,
    async run(env) {
        const sitesDir = env.or('RESTORE_SITES_DIR', 'sites');
        requireBenchDir(sitesDir);
        // ask once here; sanitize falls back to RESTORE_SITE
        await pickSite(env, 'RESTORE_SITE', sitesDir);
        banner('RESTORE');
        await restore.run(env);
        console.log();
        banner('SANITIZE');
        await sanitize.run(env);
        console.log();
        banner('RESTORE + SANITIZE COMPLETE');
    },
};
