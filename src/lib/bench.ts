import { existsSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import type { Env } from './env.js';
import { CliError } from './errors.js';
import { askSelect } from './prompt.js';

/**
 * Site-facing commands act on the bench in the CURRENT directory, not on
 * wherever the CLI is installed. Fail loudly when cwd is not a bench.
 */
export function requireBenchDir(sitesDir = 'sites'): void {
    if (!existsSync(sitesDir) || !statSync(sitesDir).isDirectory()) {
        throw new CliError(`not a bench directory: ${process.cwd()} (no '${sitesDir}/'). cd into your bench first.`);
    }
}

export function siteExists(sitesDir: string, site: string): boolean {
    return existsSync(join(sitesDir, site, 'site_config.json'));
}

/** Every entry with a site_config.json; symlinked site dirs count, as with a shell glob. */
export function listSites(sitesDir: string): string[] {
    return readdirSync(sitesDir)
        .filter((name) => siteExists(sitesDir, name))
        .sort();
}

/** Fill `name` with a site: keeps a set value, auto-picks a lone site, else asks. */
export async function pickSite(env: Env, name: string, sitesDir = 'sites'): Promise<string> {
    if (env.filled(name)) return env.get(name)!;
    const sites = listSites(sitesDir);
    if (sites.length === 0) throw new CliError(`no site found in ${sitesDir}/`);
    if (sites.length === 1) {
        env.set(name, sites[0]!);
        console.log(`Site: ${sites[0]}`);
        return sites[0]!;
    }
    return askSelect(
        env,
        name,
        'Pick site',
        sites.map((s) => ({ value: s })),
        `${sites.length} sites here, `,
    );
}
