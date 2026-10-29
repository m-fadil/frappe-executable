use crate::bench::{pick_site, require_bench_dir, site_exists};
use crate::command::{Command, logger};
use crate::env::Env;
use crate::errors::{CliError, Result};
use crate::proc::{RunOptions, run};
use std::collections::HashSet;

fn log(message: &str) {
    logger("sanitize")(message);
}

/// table → statement that switches it off; skipped when the table is missing
const DISABLE: [(&str, &str); 6] = [
    (
        "tabEmail Account",
        "UPDATE `tabEmail Account` SET enable_incoming = 0, enable_outgoing = 0, default_incoming = 0, default_outgoing = 0;",
    ),
    ("tabWebhook", "UPDATE `tabWebhook` SET enabled = 0;"),
    ("tabNotification", "UPDATE `tabNotification` SET enabled = 0;"),
    (
        "tabAuto Repeat",
        "UPDATE `tabAuto Repeat` SET status = \"Disabled\" WHERE status = \"Active\";",
    ),
    ("tabEmail Queue Recipient", "DELETE FROM `tabEmail Queue Recipient`;"),
    ("tabEmail Queue", "DELETE FROM `tabEmail Queue`;"),
];

/// table → (check name, WHERE clause still counting as enabled)
const VERIFY: [(&str, &str, &str); 5] = [
    (
        "tabEmail Account",
        "email accounts",
        "enable_incoming = 1 OR enable_outgoing = 1",
    ),
    ("tabWebhook", "webhooks", "enabled = 1"),
    ("tabNotification", "notifications", "enabled = 1"),
    ("tabAuto Repeat", "auto repeats", "status = \"Active\""),
    ("tabEmail Queue", "queued emails", "1 = 1"),
];

const DEFAULT_SINGLES: &str = "FCM Settings,TP Twilio Settings,TP Exotel Settings";

/// "a , b" -> "a,b" for FIND_IN_SET (spaces around commas only), and escape quotes.
fn normalize_singles(singles: &str) -> String {
    let pieces: Vec<&str> = singles.split(',').collect();
    let last = pieces.len() - 1;
    let joined = pieces
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let p = if i > 0 { p.trim_start_matches(' ') } else { p };
            if i < last { p.trim_end_matches(' ') } else { p }
        })
        .collect::<Vec<_>>()
        .join(",");
    joined.replace('\'', "''")
}

fn run_sanitize(env: &mut Env) -> Result<()> {
    if !env.filled("SANITIZE_SITE") && env.filled("RESTORE_SITE") {
        let site = env.get("RESTORE_SITE").unwrap_or_default().to_string();
        env.set("SANITIZE_SITE", &site);
    }
    // unset → default list; set but empty → skip the step
    let singles = env.get("SANITIZE_SINGLES").unwrap_or(DEFAULT_SINGLES).to_string();
    let sites_dir = env.or("SANITIZE_SITES_DIR", "sites");
    require_bench_dir(&sites_dir)?;
    let mute_emails = env.or("SANITIZE_MUTE_EMAILS", "1");

    let site = pick_site(env, "SANITIZE_SITE", &sites_dir)?;
    let bench = |args: &[&str], capture: bool| -> Result<String> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        run(
            env,
            "bench",
            &args,
            &RunOptions {
                capture,
                ..Default::default()
            },
        )
    };
    let mariadb = |sql: &str, capture: bool| -> Result<String> {
        let mut args = vec!["--site", &site, "mariadb"];
        if capture {
            args.extend(["-N", "-B"]);
        }
        args.extend(["-e", sql]);
        bench(&args, capture)
    };

    log(&format!("Checking site: {site}"));
    if !site_exists(&sites_dir, &site) {
        return Err(CliError::new(format!(
            "[sanitize] Site {site} does not exist under {sites_dir}/"
        )));
    }

    log("Muting email queue");
    bench(&["--site", &site, "set-config", "mute_emails", &mute_emails], false)?;

    let shown = mariadb("SHOW TABLES", true)?;
    let tables: HashSet<&str> = shown.split('\n').map(str::trim).filter(|t| !t.is_empty()).collect();

    log("Disabling outbound integrations");
    let mut statements = vec!["SET SQL_SAFE_UPDATES = 0;"];
    for (table, sql) in DISABLE {
        if tables.contains(table) {
            statements.push(sql);
        } else {
            log(&format!("skip (table missing): {table}"));
        }
    }
    mariadb(&statements.join("\n"), false)?;

    if singles.is_empty() {
        log("skip (SANITIZE_SINGLES empty)");
    } else if !tables.contains("tabSingles") {
        log("skip (table missing): tabSingles");
    } else {
        let list = normalize_singles(&singles);
        log(&format!("Disabling single doctypes: {list}"));
        mariadb(
            &format!(
                "
                SET SQL_SAFE_UPDATES = 0;

                UPDATE tabSingles
                SET value = '0'
                WHERE field = 'enabled'
                  AND FIND_IN_SET(doctype, '{list}');

                SELECT
                    'singles still enabled' AS check_name,
                    COUNT(*) AS still_enabled
                FROM tabSingles
                WHERE field = 'enabled'
                  AND value = '1';
            "
            ),
            false,
        )?;
    }

    log("Clearing cache");
    bench(&["--site", &site, "clear-cache"], false)?;

    log("Verifying remaining enabled integrations");
    let checks: Vec<String> = VERIFY
        .iter()
        .filter(|(table, ..)| tables.contains(table))
        .map(|(table, name, cond)| {
            format!("    SELECT '{name}' AS check_name, COUNT(*) AS still_enabled FROM `{table}` WHERE {cond}")
        })
        .collect();
    if !checks.is_empty() {
        mariadb(&format!("{};", checks.join("\n    UNION ALL\n")), false)?;
    }

    log("Done");
    Ok(())
}

pub static COMMAND: Command = Command {
    name: "sanitize",
    desc: "Disable outbound integrations on a restored site",
    about: "Disable outbound integrations on a restored site so it cannot mail or call out.",
    env_help: "Variables:
  SANITIZE_SITE           asked; falls back to RESTORE_SITE, auto-picked when
                          the bench has one site
  SANITIZE_SINGLES        comma-separated single doctypes to disable
                          default: FCM Settings,TP Twilio Settings,TP Exotel Settings
                          set to empty to skip
  SANITIZE_SITES_DIR      default: sites
  SANITIZE_MUTE_EMAILS    default: 1  site_config mute_emails value",
    run: run_sanitize,
};

#[cfg(test)]
mod tests {
    use super::normalize_singles;

    #[test]
    fn singles_list_strips_spaces_around_commas_and_escapes_quotes() {
        assert_eq!(normalize_singles("A , B"), "A,B");
        assert_eq!(normalize_singles("  A  ,B ,  C d "), "  A,B,C d ");
        assert_eq!(normalize_singles("O'Neil, X"), "O''Neil,X");
        assert_eq!(normalize_singles("Only One"), "Only One");
    }
}
