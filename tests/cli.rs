use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Out {
    code: i32,
    out: String,
    err: String,
}

/// Run the CLI with no terminal at all, as cron/CI would.
fn cli(args: &[&str], cwd: &Path) -> Out {
    let r = Command::new(env!("CARGO_BIN_EXE_frappe"))
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    Out {
        code: r.status.code().unwrap(),
        out: String::from_utf8_lossy(&r.stdout).into_owned(),
        err: String::from_utf8_lossy(&r.stderr).into_owned(),
    }
}

fn bench(root: &Path, name: &str, sites: &[&str]) -> PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    for s in sites {
        fs::create_dir_all(dir.join("sites").join(s)).unwrap();
        fs::write(dir.join("sites").join(s).join("site_config.json"), "{}").unwrap();
    }
    dir
}

#[test]
fn every_command_answers_help_without_a_bench() {
    let dir = tempfile::tempdir().unwrap();
    for c in ["bench-init", "restore", "sanitize", "restore-sanitize", "create-user"] {
        let r = cli(&[c, "--help"], dir.path());
        assert_eq!(r.code, 0, "{c}");
        assert!(r.out.starts_with(&format!("Usage: frappe {c} ")), "{c}: {}", r.out);
    }
}

#[test]
fn unknown_command_and_bad_option_exit_2_with_usage() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(cli(&["nope"], dir.path()).code, 2);
    let r = cli(&["restore", "--nope"], dir.path());
    assert_eq!(r.code, 2);
    assert!(
        r.err.contains("unknown option: --nope\n\nUsage: frappe restore"),
        "{}",
        r.err
    );
}

#[test]
fn version_is_the_bare_crate_version() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        cli(&["--version"], dir.path()).out,
        format!("{}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn site_commands_refuse_to_run_outside_a_bench() {
    let dir = tempfile::tempdir().unwrap();
    let r = cli(&["sanitize"], dir.path());
    assert_eq!(r.code, 1);
    assert!(r.err.contains("not a bench directory"), "{}", r.err);
}

#[test]
fn no_terminal_fails_fast_naming_what_is_missing() {
    let root = tempfile::tempdir().unwrap();
    let two = bench(root.path(), "two", &["a.localhost", "b.localhost"]);
    assert!(
        cli(&["restore"], &two)
            .err
            .contains("RESTORE_SITE is required (2 sites here, no terminal")
    );

    let one = bench(root.path(), "one", &["a.localhost"]);
    let r = cli(&["restore", "-e", "RESTORE_SOURCE_URL=https://x"], &one);
    assert!(r.out.contains("Site: a.localhost"), "{}", r.out);
    assert!(
        r.err.contains("RESTORE_SOURCE_TOKEN is required (no terminal"),
        "{}",
        r.err
    );

    assert!(cli(&["create-user"], &one).err.contains("USER_EMAIL is required"));
    let d = root.path();
    assert!(
        cli(&["bench-init"], d)
            .err
            .contains("MODE is required when running non-interactively")
    );
    assert!(
        cli(&["bench-init", "-e", "MODE=server"], d)
            .err
            .contains("no tty: set YES=1")
    );
    assert!(
        cli(&["bench-init", "-e", "MODE=server", "-e", "YES=1"], d)
            .err
            .contains("missing required: DB_ROOT_PASS ADMIN_PASS")
    );
    assert!(
        cli(&["bench-init", "-e", "MODE=k8s", "-e", "YES=1"], d)
            .err
            .contains("MODE must be 'server' or 'docker'")
    );
}
