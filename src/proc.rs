use crate::env::Env;
use crate::errors::{CliError, Result};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Default)]
pub struct RunOptions {
    pub cwd: Option<PathBuf>,
    /// extra variables for this child only
    pub extra_env: Vec<(String, String)>,
    /// collect stdout instead of passing it through
    pub capture: bool,
}

pub struct RunResult {
    pub code: i32,
    pub stdout: String,
}

extern "C" fn on_sigint(_: libc::c_int) {}

/// Restores the previous SIGINT disposition on drop.
struct SigintGuard(libc::sighandler_t);

impl SigintGuard {
    /// Ctrl+C reaches the child too (same process group); outlive it like a
    /// shell would, so its cleanup output lands before we exit. A handler, not
    /// SIG_IGN: ignored dispositions survive exec, handled ones reset to default.
    fn install() -> Self {
        let handler: extern "C" fn(libc::c_int) = on_sigint;
        // SAFETY: on_sigint is async-signal-safe (empty).
        Self(unsafe { libc::signal(libc::SIGINT, handler as libc::sighandler_t) })
    }
}

impl Drop for SigintGuard {
    fn drop(&mut self) {
        // SAFETY: restoring the disposition returned by signal().
        unsafe { libc::signal(libc::SIGINT, self.0) };
    }
}

/// Spawn and wait. stdin/stderr are always the terminal's. A child killed by
/// Ctrl+C fails with exit 130 so the whole command stops, like a shell script.
/// Error messages name only the program: args can hold passwords.
pub fn spawn_wait(env: &Env, cmd: &str, args: &[String], opts: &RunOptions) -> Result<RunResult> {
    let mut command = Command::new(cmd);
    command
        .args(args)
        .env_clear()
        .envs(env.vars())
        .envs(opts.extra_env.iter().map(|(k, v)| (k, v)));
    if let Some(cwd) = &opts.cwd {
        command.current_dir(cwd);
    }
    command.stdin(Stdio::inherit()).stderr(Stdio::inherit());
    command.stdout(if opts.capture { Stdio::piped() } else { Stdio::inherit() });

    let _guard = SigintGuard::install();
    let child = command
        .spawn()
        .map_err(|e| CliError::new(format!("cannot run {cmd}: {e}")))?;
    let out = child
        .wait_with_output()
        .map_err(|e| CliError::new(format!("cannot run {cmd}: {e}")))?;
    drop(_guard);

    if out.status.signal() == Some(libc::SIGINT) {
        return Err(CliError::with_code("", 130));
    }
    if let Some(n) = out.status.signal() {
        return Err(CliError::new(format!("{cmd} killed by signal {n}")));
    }
    Ok(RunResult {
        code: out.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
    })
}

/// Run to completion; any non-zero exit becomes a CliError carrying that code.
pub fn run(env: &Env, cmd: &str, args: &[String], opts: &RunOptions) -> Result<String> {
    let r = spawn_wait(env, cmd, args, opts)?;
    if r.code != 0 {
        return Err(CliError::with_code(
            format!("{cmd} exited with code {}", r.code),
            r.code as u8,
        ));
    }
    Ok(r.stdout)
}

/// Absolute path of an executable on PATH, like `command -v`.
pub fn which(cmd: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|d| !d.as_os_str().is_empty())
        .map(|d| d.join(cmd))
        .find(|full| {
            std::fs::metadata(full)
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        })
}
