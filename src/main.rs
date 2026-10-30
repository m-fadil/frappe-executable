mod backups;
mod bench;
mod command;
mod commands;
mod download;
mod env;
mod envfile;
mod errors;
mod proc;
mod prompt;
mod update;

use command::{Command, PROG, help_text};
use commands::COMMANDS;
use env::{Env, load_env, parse_args};
use errors::{CliError, Result};
use std::io::Write;

fn usage() -> String {
    let row = |name: &str, desc: &str| format!("  {name:<18} {desc}");
    let mut lines: Vec<String> = vec![
        format!("Usage: {PROG} <command> [options] [env-file...]"),
        String::new(),
        "Commands:".into(),
    ];
    lines.extend(COMMANDS.iter().map(|c| row(c.name, c.desc)));
    lines.extend([
        String::new(),
        "Self:".into(),
        row("update", "Update to the latest release (--check to only look)"),
        row("uninstall", "Remove this binary"),
        row("--version, -v", "Show the installed version"),
        String::new(),
        "Every command accepts -h/--help, -e VAR=val and --env-file PATH.".into(),
        format!("Run \"{PROG} <command> --help\" for its variables."),
        String::new(),
        "Commands act on the bench in the CURRENT directory.".into(),
        String::new(),
    ]);
    lines.join("\n")
}

/// Path of the running install. A cargo build artifact is not an install.
fn installed_exe(action: &str) -> Result<std::path::PathBuf> {
    let exe = std::fs::canonicalize(std::env::current_exe()?)?;
    let in_target = exe.ancestors().skip(1).any(|a| {
        a.file_name().is_some_and(|n| n == "target") && a.parent().is_some_and(|p| p.join("Cargo.toml").is_file())
    });
    if in_target {
        return Err(CliError::new(format!(
            "{} is a cargo build artifact, not an install — {action}",
            exe.display()
        )));
    }
    Ok(exe)
}

/// Remove the running binary.
fn uninstall() -> Result<()> {
    let exe = installed_exe("delete it with rm instead")?;
    let dir = exe.parent().map(|p| p.display().to_string()).unwrap_or_default();
    std::fs::remove_file(&exe).map_err(|e| {
        CliError::new(format!(
            "cannot remove {}: {e}\nif {dir} needs root: sudo rm {}",
            exe.display(),
            exe.display()
        ))
    })?;
    println!("uninstalled {PROG} from {dir}");
    Ok(())
}

/// Ok(exit code) for outcomes that are not failures (usage errors exit 2).
fn real_main(argv: Vec<String>) -> Result<u8> {
    let name = argv.first().map(String::as_str);
    let rest = argv.get(1..).unwrap_or(&[]);
    match name {
        None | Some("-h" | "--help" | "help") => {
            print!("{}", usage());
            return Ok(0);
        }
        Some("--version" | "-V" | "-v") => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            return Ok(0);
        }
        Some("update") => return update::run(rest, || installed_exe("rebuild it with cargo instead")),
        Some("uninstall") => {
            uninstall()?;
            return Ok(0);
        }
        _ => {}
    }

    let name = name.unwrap_or_default();
    let Some(cmd): Option<&&Command> = COMMANDS.iter().find(|c| c.name == name) else {
        eprint!("unknown command: {name}\n\n{}", usage());
        return Ok(2);
    };

    let args = match parse_args(rest) {
        Ok(a) => a,
        Err(e) => {
            eprint!("{}\n\n{}", e.0, help_text(cmd));
            return Ok(2);
        }
    };
    if args.help {
        print!("{}", help_text(cmd));
        return Ok(0);
    }
    let mut env: Env = load_env(&args, Env::from_process())?;
    (cmd.run)(&mut env)?;
    Ok(0)
}

fn main() {
    // SAFETY: restoring the default SIGPIPE disposition so `| head` ends us quietly instead of panicking.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let argv: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let code = match real_main(argv) {
        Ok(code) => code,
        Err(e) => {
            if !e.message.is_empty() {
                eprintln!("{}", e.message);
            }
            e.code
        }
    };
    let _ = std::io::stdout().flush();
    std::process::exit(i32::from(code));
}
