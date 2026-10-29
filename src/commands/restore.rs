use crate::backups::{check_backup_path, expand_path, format_backup, list_backups};
use crate::bench::{pick_site, require_bench_dir, site_exists};
use crate::command::{Command, logger};
use crate::download::{DownloadOptions, agent, download_resumable, gzip_intact};
use crate::env::Env;
use crate::errors::{CliError, Result};
use crate::proc::{RunOptions, run, spawn_wait, which};
use crate::prompt::{Choice, ask_secret, ask_var, can_prompt, prompt_search, prompt_select};
use std::fs;
use std::path::{Path, PathBuf};

fn log(message: &str) {
    logger("restore")(message);
}

fn latest_backup_path(source_url: &str, auth: &str) -> Result<String> {
    let url = format!("{source_url}/api/method/frappe.utils.backups.fetch_latest_backups");
    let resp = agent()
        .get(&url)
        .header("Authorization", auth)
        .call()
        .map_err(|e| CliError::new(format!("[restore] cannot reach {url}: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        let reason = status.canonical_reason().unwrap_or("");
        return Err(CliError::new(format!(
            "[restore] {url} answered HTTP {} {reason}",
            status.as_u16()
        )));
    }
    let not_json = || CliError::new(format!("[restore] {url} did not return JSON"));
    let text = resp.into_body().read_to_string().map_err(|_| not_json())?;
    let body: serde_json::Value = serde_json::from_str(&text).map_err(|_| not_json())?;
    match body.pointer("/message/database").and_then(|v| v.as_str()) {
        Some(db) if !db.is_empty() => Ok(db.to_string()),
        _ => Err(CliError::new("[restore] Source site returned no database backup")),
    }
}

/// aria2c with N connections; exit 13 means "already fully downloaded".
fn download_aria2(env: &Env, url: &str, auth: &str, dir: &Path, file: &str, connections: &str) -> Result<()> {
    log(&format!(
        "Downloading with aria2c ({connections} connections) into {}",
        dir.display()
    ));
    let args: Vec<String> = [
        "--no-conf",
        "-x",
        connections,
        "-s",
        connections,
        "-c",
        "--auto-file-renaming=false",
        "--allow-overwrite=false",
        &format!("--header={auth}"),
        "-d",
        &dir.to_string_lossy(),
        "-o",
        file,
        url,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let r = spawn_wait(env, "aria2c", &args, &RunOptions::default())?;
    if r.code != 0 && r.code != 13 {
        return Err(CliError::with_code(
            format!("[restore] aria2c failed (exit {})", r.code),
            r.code as u8,
        ));
    }
    Ok(())
}

/// Latest backup of the source site, downloaded (or resumed) into RESTORE_DOWNLOAD_DIR.
fn download_latest(env: &Env, source_url: &str, token: &str) -> Result<PathBuf> {
    let download_dir = std::path::absolute(env.or("RESTORE_DOWNLOAD_DIR", &cwd_string()))?;
    let connections = env.or("RESTORE_CONNECTIONS", "4");
    fs::create_dir_all(&download_dir)?;

    log(&format!("Fetching latest backup metadata from {source_url}"));
    let remote = latest_backup_path(source_url, token)?;
    let backup_file = remote.rsplit('/').next().unwrap_or(&remote).to_string();
    let backup_path = download_dir.join(&backup_file);
    let backup_url = format!("{source_url}/backups/{backup_file}");
    log(&format!("Latest database backup: {backup_file}"));

    let mut aria_os = backup_path.as_os_str().to_owned();
    aria_os.push(".aria2");
    if backup_path.exists() && !Path::new(&aria_os).exists() {
        if gzip_intact(&backup_path) {
            log("Backup already downloaded, skipping download");
            return Ok(backup_path);
        }
        log("Existing file is corrupt, re-downloading");
        let _ = fs::remove_file(&backup_path);
    }

    if which("aria2c").is_some() {
        download_aria2(
            env,
            &backup_url,
            &format!("Authorization: {token}"),
            &download_dir,
            &backup_file,
            &connections,
        )?;
    } else {
        log(&format!(
            "aria2c not found, downloading over a single connection into {}",
            download_dir.display()
        ));
        let opts = DownloadOptions {
            headers: vec![("Authorization".into(), token.to_string())],
            log: &log,
            attempts: 4,
            backoff_ms: 1000,
            progress: std::io::IsTerminal::is_terminal(&std::io::stderr()),
        };
        download_resumable(&backup_url, &backup_path, &opts)?;
    }
    if !backup_path.exists() {
        return Err(CliError::new("[restore] Backup download failed"));
    }
    Ok(backup_path)
}

fn cwd_string() -> String {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}

/// A file is used as is; a directory offers its backups newest first (no terminal: the newest).
fn pick_local_backup(from: &str) -> Result<PathBuf> {
    let path = expand_path(from, &cwd());
    check_backup_path(&path).map_err(|p| CliError::new(format!("[restore] RESTORE_FROM: {p}")))?;
    if !fs::metadata(&path)?.is_dir() {
        return Ok(path);
    }

    let backups = list_backups(&path);
    let newest = &backups[0];
    if backups.len() == 1 || !can_prompt() {
        log(&format!(
            "Newest of {} backup(s) in {}: {}",
            backups.len(),
            path.display(),
            newest.name
        ));
        return Ok(newest.path.clone());
    }
    let choices: Vec<Choice> = backups
        .iter()
        .map(|b| Choice::named(b.path.to_string_lossy(), format_backup(b)))
        .collect();
    let picked = prompt_select(
        &format!("Backup in {} (newest first)", path.display()),
        &choices,
        Some(&newest.path.to_string_lossy()),
    )?;
    Ok(PathBuf::from(picked))
}

enum Source {
    Local(PathBuf),
    Url { url: String, token: String },
}

fn validate_typed_path(typed: &str) -> std::result::Result<(), String> {
    check_backup_path(&expand_path(typed, &cwd()))
}

/// RESTORE_FROM wins over RESTORE_SOURCE_URL; with neither set, ask which.
fn choose_source(env: &mut Env) -> Result<Source> {
    if !env.filled("RESTORE_FROM") && !env.filled("RESTORE_SOURCE_URL") {
        if !can_prompt() {
            return Err(CliError::new(
                "RESTORE_SOURCE_URL or RESTORE_FROM is required (no terminal to ask on; pass -e NAME=... or --env-file)",
            ));
        }
        let mode = prompt_select(
            "Restore from",
            &[
                Choice::named("download", "Download the latest backup from a source site"),
                Choice::named("local", "A local backup file, or a directory of backups"),
            ],
            None,
        )?;
        if mode == "local" {
            let typed = prompt_search("Backup file or directory (Tab completes)", validate_typed_path)?;
            env.set("RESTORE_FROM", &typed);
        }
    }
    if env.filled("RESTORE_FROM") {
        let from = env.get("RESTORE_FROM").unwrap_or_default().to_string();
        return Ok(Source::Local(pick_local_backup(&from)?));
    }

    let url = ask_var(env, "RESTORE_SOURCE_URL", "Source site URL (https://...)", "")?;
    let url = url.trim_end_matches('/').to_string();
    let token = ask_secret(
        env,
        "RESTORE_SOURCE_TOKEN",
        "Source Authorization header (e.g. 'token key:secret')",
    )?;
    Ok(Source::Url { url, token })
}

pub fn run_restore(env: &mut Env) -> Result<()> {
    // DB_ROOT_PASS is the shared name (bench-init uses it too); DB_PASSWORD is
    // kept as an alias so older env files keep working.
    if !env.filled("DB_ROOT_PASS") && env.filled("DB_PASSWORD") {
        let alias = env.get("DB_PASSWORD").unwrap_or_default().to_string();
        env.set("DB_ROOT_PASS", &alias);
    }

    let sites_dir = env.or("RESTORE_SITES_DIR", "sites");
    require_bench_dir(&sites_dir)?;

    // anything still unset is asked for, so this runs fine with no env file
    let site = pick_site(env, "RESTORE_SITE", &sites_dir)?;
    let source = choose_source(env)?;
    let db_user = ask_var(env, "DB_ROOT_USER", "Database root user", "root")?;
    let db_pass = ask_secret(env, "DB_ROOT_PASS", "Database root password")?;
    let admin_pass = ask_secret(env, "RESTORE_ADMIN_PASSWORD", "Admin password to set after restore")?;

    log(&format!("Checking site: {site}"));
    if !site_exists(&sites_dir, &site) {
        return Err(CliError::new(format!(
            "[restore] Site {site} does not exist under {sites_dir}/\n[restore] Create the site first"
        )));
    }

    let backup_path = match &source {
        Source::Url { url, token } => download_latest(env, url, token)?,
        Source::Local(path) => {
            log(&format!("Using local backup: {}", path.display()));
            if path.to_string_lossy().ends_with(".gz") {
                log("Checking backup integrity");
                if !gzip_intact(path) {
                    return Err(CliError::new(format!(
                        "[restore] {} is corrupt (gzip check failed)",
                        path.display()
                    )));
                }
            }
            path.clone()
        }
    };

    let bench = |args: &[&str]| -> Result<()> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        run(env, "bench", &args, &RunOptions::default()).map(|_| ())
    };

    log(&format!("Restoring into {site}"));
    log("WARNING: existing database data will be dropped");
    bench(&[
        "--site",
        &site,
        "restore",
        &backup_path.to_string_lossy(),
        "--db-root-username",
        &db_user,
        "--db-root-password",
        &db_pass,
        "--force",
    ])?;

    if env.filled("RESTORE_SKIP_MIGRATE") {
        log("Skipping migrate (RESTORE_SKIP_MIGRATE set)");
    } else {
        log(&format!("Migrating {site}"));
        bench(&["--site", &site, "migrate"])?;
    }

    log("Setting admin password");
    bench(&["--site", &site, "set-admin-password", &admin_pass])?;

    // a local backup is the user's file: never delete it
    if matches!(source, Source::Url { .. }) {
        if env.or("RESTORE_KEEP_BACKUP", "1") == "0" {
            let _ = fs::remove_file(&backup_path);
            log("Backup deleted");
        } else {
            log(&format!(
                "Backup kept at {} (delete manually when no longer needed)",
                backup_path.display()
            ));
        }
    }
    log("Done");
    Ok(())
}

pub static COMMAND: Command = Command {
    name: "restore",
    desc: "Restore the latest backup from a source site or a local path",
    about: "Restore a database backup into a local site: either the latest backup of a
source site (downloaded) or a local .sql / .sql.gz file or directory of them.",
    env_help: "Anything required and unset is asked for on the terminal, so no env file is
needed. Passwords are read without echo. With neither RESTORE_FROM nor
RESTORE_SOURCE_URL set, you are asked which source to use.

Variables:
  RESTORE_SITE            asked; auto-picked when the bench has one site
  RESTORE_FROM            local .sql/.sql.gz file, or a directory to pick from
                          (newest first; no terminal: the newest). Wins over
                          RESTORE_SOURCE_URL. Never deleted.
  RESTORE_SOURCE_URL      asked unless RESTORE_FROM is set
  RESTORE_SOURCE_TOKEN    asked unless RESTORE_FROM is set; Authorization header value
  DB_ROOT_USER            asked  default: root
  DB_ROOT_PASS            asked  database root password (alias: DB_PASSWORD)
  RESTORE_ADMIN_PASSWORD  asked  admin password to set after restore
  RESTORE_DOWNLOAD_DIR    default: current directory
  RESTORE_SITES_DIR       default: sites
  RESTORE_CONNECTIONS     default: 4        aria2c -x/-s; unused without aria2c
  RESTORE_KEEP_BACKUP     default: 1        set to 0 to delete a downloaded backup after restore
  RESTORE_SKIP_MIGRATE    default: empty    non-empty skips 'bench migrate'",
    run: run_restore,
};
