use crate::bench::{pick_site, require_bench_dir, site_exists};
use crate::command::Command;
use crate::env::Env;
use crate::errors::{CliError, Result};
use crate::proc::{RunOptions, run};
use crate::prompt::ask_var;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

// values come in via the environment, never interpolated into the source
const SCRIPT: &str = r##"import os

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
"##;

/// `@` followed somewhere later (same line) by `.`
fn looks_like_email(s: &str) -> bool {
    s.lines()
        .any(|line| line.find('@').is_some_and(|at| line[at..].contains('.')))
}

fn run_create_user(env: &mut Env) -> Result<()> {
    let sites_dir = env.or("USER_SITES_DIR", "sites");
    require_bench_dir(&sites_dir)?;

    let site = pick_site(env, "USER_SITE", &sites_dir)?;
    if !site_exists(&sites_dir, &site) {
        return Err(CliError::new(format!("Site {site} does not exist under {sites_dir}/")));
    }

    let email = ask_var(env, "USER_EMAIL", "User email", "")?;
    if !looks_like_email(&email) {
        return Err(CliError::new(format!("not an email address: {email}")));
    }
    let local_part = email.split('@').next().unwrap_or_default().to_string();
    let first_name = ask_var(env, "USER_FIRST_NAME", "First name", &local_part)?;

    let last_name = env.or("USER_LAST_NAME", "");
    // unset → System Manager; set but empty → no roles
    let roles = env.get("USER_ROLES").unwrap_or("System Manager").to_string();
    let api_key = env.or("USER_API_KEY", "1");
    let vars: Vec<(String, String)> = vec![
        ("USER_EMAIL".into(), email.clone()),
        ("USER_FIRST_NAME".into(), first_name.clone()),
        ("USER_LAST_NAME".into(), last_name.clone()),
        ("USER_ROLES".into(), roles.clone()),
        ("USER_PASSWORD".into(), env.or("USER_PASSWORD", "")),
        ("USER_API_KEY".into(), api_key.clone()),
        ("USER_WELCOME_EMAIL".into(), env.or("USER_WELCOME_EMAIL", "0")),
    ];

    let name: Vec<&str> = [first_name.as_str(), last_name.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    println!("Site       : {site}");
    println!("Email      : {email}");
    println!("Name       : {}", name.join(" "));
    println!("Roles      : {}", if roles.is_empty() { "<none>" } else { &roles });
    println!("API key    : {}", if api_key == "0" { "no" } else { "yes" });
    println!();

    // removed on drop, whatever way we leave
    let dir = tempfile::Builder::new().prefix("frappe-create-user-").tempdir()?;
    let script = dir.path().join("create_user.py");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&script)?
        .write_all(SCRIPT.as_bytes())?;
    // a JSON string is a valid Python string literal
    let path = serde_json::to_string(&script.to_string_lossy()).map_err(|e| CliError::new(e.to_string()))?;
    let args: Vec<String> = vec![
        "--site".into(),
        site,
        "execute".into(),
        format!("exec(compile(open({path}).read(), {path}, 'exec'))"),
    ];
    run(
        env,
        "bench",
        &args,
        &RunOptions {
            extra_env: vars,
            ..Default::default()
        },
    )?;
    Ok(())
}

pub static COMMAND: Command = Command {
    name: "create-user",
    desc: "Create a user on a site, optionally print API credentials",
    about: "Create (or reuse) a user on a site in the current bench.",
    env_help: "Variables:
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
  frappe create-user -e USER_EMAIL=mcp@example.com
  frappe create-user -e USER_EMAIL=ops@example.com -e USER_FIRST_NAME=Ops \\
      -e USER_ROLES='Accounts User,Sales User' -e USER_API_KEY=0
  frappe create-user --env-file user.env",
    run: run_create_user,
};
