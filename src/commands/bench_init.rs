use crate::command::{Color, Command, heading};
use crate::env::Env;
use crate::errors::{CliError, Result};
use crate::proc::{RunOptions, run};
use crate::prompt::{Choice, ask_select, can_prompt, prompt_confirm, prompt_secret, prompt_select, prompt_text};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Server,
    Docker,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::Server => "server",
            Mode::Docker => "docker",
        }
    }
}

struct Field {
    key: &'static str,
    label: &'static str,
    secret: bool,
    required: bool,
}

const fn field(key: &'static str, label: &'static str) -> Field {
    Field {
        key,
        label,
        secret: false,
        required: false,
    }
}

const fn secret(key: &'static str, label: &'static str) -> Field {
    Field {
        key,
        label,
        secret: true,
        required: true,
    }
}

const COMMON_FIELDS: [Field; 9] = [
    field("FRAPPE_VERSION", "Frappe Version"),
    field("BENCH_NAME", "Bench Name"),
    field("SITE_NAME", "Site Name"),
    field("DB_HOST", "DB Host"),
    field("DB_PORT", "DB Port"),
    field("DB_NAME", "DB Name"),
    field("DB_ROOT_USER", "DB Root User"),
    secret("DB_ROOT_PASS", "DB Root Password"),
    secret("ADMIN_PASS", "Admin Password"),
];

const DOCKER_FIELDS: [Field; 3] = [
    field("REDIS_CACHE", "Redis Cache"),
    field("REDIS_QUEUE", "Redis Queue"),
    field("REDIS_SOCKETIO", "Redis SocketIO"),
];

/// Default DB name for a site: every non-alphanumeric becomes `_`.
fn db_name_for(site: &str) -> String {
    site.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Fill every unset field with its mode-specific default.
fn apply_defaults(env: &mut Env, mode: Mode) {
    let site = env.or("SITE_NAME", "development.localhost");
    let docker = mode == Mode::Docker;
    let mut defaults: Vec<(&str, String)> = vec![
        ("FRAPPE_VERSION", "version-16".into()),
        ("BENCH_NAME", "frappe-bench".into()),
        ("SITE_NAME", site.clone()),
        ("DB_HOST", if docker { "mariadb" } else { "127.0.0.1" }.into()),
        ("DB_PORT", "3306".into()),
        ("DB_NAME", db_name_for(&site)),
        ("DB_ROOT_USER", "root".into()),
    ];
    if docker {
        defaults.push(("REDIS_CACHE", "redis://redis-cache:6379".into()));
        defaults.push(("REDIS_QUEUE", "redis://redis-queue:6379".into()));
        defaults.push(("REDIS_SOCKETIO", "redis://redis-socketio:6379".into()));
    }
    for (key, value) in defaults {
        let v = env.or(key, &value);
        env.set(key, &v);
    }
}

fn display(env: &Env, field: &Field) -> String {
    if !env.filled(field.key) {
        "<not set>".into()
    } else if field.secret {
        "••••••••".into()
    } else {
        env.get(field.key).unwrap_or_default().to_string()
    }
}

fn missing_required<'a>(env: &Env, fields: &[&'a Field]) -> Vec<&'a str> {
    fields
        .iter()
        .filter(|f| f.required && !env.filled(f.key))
        .map(|f| f.key)
        .collect()
}

fn edit_field(env: &mut Env, field: &Field) -> Result<()> {
    if field.secret {
        // an empty answer keeps a secret that is already set
        let keep = env.filled(field.key);
        let message = if keep {
            format!("{} (empty keeps current)", field.label)
        } else {
            field.label.to_string()
        };
        let value = prompt_secret(&message, keep)?;
        if !value.is_empty() {
            env.set(field.key, &value);
        }
        return Ok(());
    }
    let current = env.get(field.key).map(String::from);
    let value = prompt_text(field.label, current.as_deref(), true)?;
    env.set(field.key, &value);
    Ok(())
}

const APPLY: &str = "__apply__";
const QUIT: &str = "__quit__";

/// The config menu: pick a field to edit, or apply / quit. Returns true to run.
fn configure(env: &mut Env, mode: Mode, fields: &[&Field]) -> Result<bool> {
    for field in fields {
        if field.required && !env.filled(field.key) {
            edit_field(env, field)?;
        }
    }

    let width = fields.iter().map(|f| f.label.chars().count()).max().unwrap_or(0) + 2;
    let mut cursor = APPLY.to_string();
    loop {
        let mut choices: Vec<Choice> = fields
            .iter()
            .map(|f| Choice::named(f.key, format!("{:<width$}{}", f.label, display(env, f))))
            .collect();
        choices.push(Choice::named(APPLY, "Apply & run"));
        choices.push(Choice::named(QUIT, "Quit"));
        cursor = prompt_select(
            &format!("Frappe bench init (mode: {}) — pick a value to change", mode.as_str()),
            &choices,
            Some(&cursor),
        )?;
        if cursor == QUIT {
            return Ok(false);
        }
        if cursor != APPLY {
            // DB Name tracks Site Name until it is given a value of its own
            let follows = cursor == "SITE_NAME"
                && env.get("DB_NAME") == Some(db_name_for(env.get("SITE_NAME").unwrap_or_default()).as_str());
            if let Some(field) = fields.iter().find(|f| f.key == cursor) {
                edit_field(env, field)?;
            }
            if follows {
                let name = db_name_for(env.get("SITE_NAME").unwrap_or_default());
                env.set("DB_NAME", &name);
            }
            continue;
        }
        let missing = missing_required(env, fields);
        if !missing.is_empty() {
            eprintln!("Still missing: {}", missing.join(", "));
            continue;
        }
        if prompt_confirm("Run bench init now?", true)? {
            return Ok(true);
        }
    }
}

fn run_bench(env: &Env, mode: Mode) -> Result<()> {
    let v = |k: &str| env.need(k);
    let words = |k: &str| -> Vec<String> {
        env.get(k)
            .unwrap_or_default()
            .split_whitespace()
            .map(String::from)
            .collect()
    };
    let docker = mode == Mode::Docker;

    let mut init_args: Vec<String> = vec![
        "init".into(),
        "--ignore-exist".into(),
        format!("--frappe-branch={}", v("FRAPPE_VERSION")?),
        "--dev".into(),
    ];
    let mut site_args: Vec<String> = vec![
        "new-site".into(),
        format!("--db-name={}", v("DB_NAME")?),
        format!("--db-root-username={}", v("DB_ROOT_USER")?),
        format!("--db-root-password={}", v("DB_ROOT_PASS")?),
        format!("--admin-password={}", v("ADMIN_PASS")?),
    ];
    if docker {
        init_args.push("--skip-redis-config-generation".into());
        site_args.push("--mariadb-user-host-login-scope=%".into());
    }
    init_args.extend(words("BENCH_INIT_EXTRA"));
    init_args.push(v("BENCH_NAME")?);
    site_args.extend(words("BENCH_NEW_SITE_EXTRA"));
    site_args.push(v("SITE_NAME")?);

    let none = RunOptions::default();
    heading(
        &format!("\n== Running bench init (mode: {}) ==\n", mode.as_str()),
        Color::Cyan,
    );

    heading("[1/3] Initialising bench...", Color::Yellow);
    run(env, "bench", &init_args, &none)?;

    let in_bench = || RunOptions {
        cwd: v("BENCH_NAME").ok().map(PathBuf::from),
        ..Default::default()
    };
    heading("\n[2/3] Setting global config...", Color::Yellow);
    let set_config = |key: &str, value: String| -> Result<()> {
        let args = vec!["set-config".to_string(), "-g".into(), key.into(), value];
        run(env, "bench", &args, &in_bench()).map(|_| ())
    };
    set_config("db_host", v("DB_HOST")?)?;
    set_config("db_port", v("DB_PORT")?)?;
    if docker {
        for key in ["redis_cache", "redis_queue", "redis_socketio"] {
            set_config(key, v(&key.to_uppercase())?)?;
        }
    } else {
        println!("  (server mode: bench generates its own redis config)");
    }

    heading("\n[3/3] Creating site...", Color::Yellow);
    run(env, "bench", &site_args, &in_bench())?;

    heading("\n✔ Setup complete\n", Color::Green);
    Ok(())
}

fn run_bench_init(env: &mut Env) -> Result<()> {
    let unattended = env.filled("YES");
    let interactive = !unattended && can_prompt();

    if !env.filled("MODE") && !interactive {
        return Err(CliError::new(
            "MODE is required when running non-interactively (-e MODE=server|docker)",
        ));
    }
    let mode_text = ask_select(
        env,
        "MODE",
        "Target deployment",
        &[
            Choice::described("server", "bench manages redis itself, DB on 127.0.0.1"),
            Choice::described("docker", "redis & mariadb as separate services (devcontainer)"),
        ],
        "",
    )?;
    let mode = match mode_text.as_str() {
        "server" => Mode::Server,
        "docker" => Mode::Docker,
        other => {
            return Err(CliError::new(format!(
                "MODE must be 'server' or 'docker' (got: {other})"
            )));
        }
    };

    apply_defaults(env, mode);
    let mut fields: Vec<&Field> = COMMON_FIELDS.iter().collect();
    if mode == Mode::Docker {
        fields.extend(DOCKER_FIELDS.iter());
    }

    if unattended {
        let missing = missing_required(env, &fields);
        if !missing.is_empty() {
            return Err(CliError::new(format!("missing required: {}", missing.join(" "))));
        }
    } else {
        if !interactive {
            return Err(CliError::new("no tty: set YES=1 to run non-interactively"));
        }
        if !configure(env, mode, &fields)? {
            println!("Cancelled.");
            return Ok(());
        }
    }
    run_bench(env, mode)
}

pub static COMMAND: Command = Command {
    name: "bench-init",
    desc: "Init a new bench + site (server or docker layout)",
    about: "Interactive Frappe bench init: pick a layout, review every value, then run.",
    env_help: "Variables:
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
--mariadb-user-host-login-scope=%. Server mode does none of those.",
    run: run_bench_init,
};
