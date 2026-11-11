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

fn env_help() -> String {
    format!(
        "This runs restore, then sanitize, in one merged environment. Anything
required and unset is asked for on the terminal, so no env file is needed.
The target site is picked once for the restore step (RESTORE_SITE); the
sanitize step uses SANITIZE_SITE when set, otherwise the same site.

Restore step:
{}

Sanitize step:
{}",
        restore::ENV_VARS,
        sanitize::ENV_VARS
    )
}

pub static COMMAND: Command = Command {
    name: "restore-sanitize",
    desc: "Run restore then sanitize in one go",
    about: "Run restore then sanitize with the same options.",
    env_help,
    run,
};
