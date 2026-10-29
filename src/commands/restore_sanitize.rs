use super::{restore, sanitize};
use crate::bench::{pick_site, require_bench_dir};
use crate::command::Command;
use crate::env::Env;
use crate::errors::Result;

fn banner(title: &str) {
    println!("========================================\n {title}\n========================================");
}

fn run(env: &mut Env) -> Result<()> {
    let sites_dir = env.or("RESTORE_SITES_DIR", "sites");
    require_bench_dir(&sites_dir)?;
    // ask once here; sanitize falls back to RESTORE_SITE
    pick_site(env, "RESTORE_SITE", &sites_dir)?;

    banner("RESTORE");
    (restore::COMMAND.run)(env)?;
    println!();
    banner("SANITIZE");
    (sanitize::COMMAND.run)(env)?;
    println!();
    banner("RESTORE + SANITIZE COMPLETE");
    Ok(())
}

pub static COMMAND: Command = Command {
    name: "restore-sanitize",
    desc: "Run restore then sanitize in one go",
    about: "Run restore then sanitize with the same options.",
    env_help: "No env file is required: anything missing is asked for on the terminal.
Both steps read the same variables, so see 'frappe restore --help' and
'frappe sanitize --help'. The site is asked for once and used by both.",
    run,
};
