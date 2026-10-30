use crate::command::PROG;
use crate::download::agent;
use crate::errors::{CliError, Result};
use std::path::PathBuf;
use std::process::Command;
use ureq::ResponseExt;

pub const HELP: &str = "\
Usage: frappe update [--check] [VERSION]

Update this binary to the latest stable release, or to VERSION (e.g. v2.0.1).
Runs the installer of that release, which verifies the SHA-256 checksum and
replaces the binary in place. Needs curl or wget and sh.

Options:
  --check     only report whether a newer release exists, do not install
  -h, --help  show this help

Environment:
  FRAPPE_RELEASES_URL  releases base URL (default: the GitHub Releases page)
";

const DEFAULT_RELEASES: &str = "https://github.com/m-fadil/frappe-executable/releases";

struct Args {
    check: bool,
    version: Option<String>,
}

/// Err carries the usage message; `-h/--help` is handled by the caller.
fn parse(args: &[String]) -> std::result::Result<Args, String> {
    let mut res = Args {
        check: false,
        version: None,
    };
    for a in args {
        match a.as_str() {
            "--check" => res.check = true,
            s if s.starts_with('-') => return Err(format!("unknown option: {s}")),
            s if res.version.is_none() => res.version = Some(normalize(s)),
            s => return Err(format!("unexpected argument: {s}")),
        }
    }
    Ok(res)
}

fn normalize(v: &str) -> String {
    if v.starts_with('v') {
        v.to_string()
    } else {
        format!("v{v}")
    }
}

/// `https://…/releases/tag/v2.0.1` → `v2.0.1`
fn tag_of(url: &str) -> Option<&str> {
    let tag = url.split_once("/tag/")?.1;
    let tag = tag.split(['?', '#']).next()?;
    tag.starts_with('v').then_some(tag)
}

fn latest_tag(releases: &str) -> Result<String> {
    let resp = agent()
        .get(format!("{releases}/latest"))
        .call()
        .map_err(|e| CliError::new(format!("cannot look up the latest release: {e}")))?;
    let url = resp.get_uri().to_string();
    match tag_of(&url) {
        Some(tag) => Ok(tag.to_string()),
        None => Err(CliError::new(format!(
            "no stable release found (got {url}). Pick one: {PROG} update vX.Y.Z"
        ))),
    }
}

fn fetch_text(url: &str) -> Result<String> {
    let mut resp = agent()
        .get(url)
        .call()
        .map_err(|e| CliError::new(format!("download failed: {e}")))?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(CliError::new(format!("download failed: HTTP {status} for {url}")));
    }
    resp.body_mut()
        .read_to_string()
        .map_err(|e| CliError::new(format!("download failed: {e}")))
}

pub fn run(args: &[String], exe: impl FnOnce() -> Result<PathBuf>) -> Result<u8> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return Ok(0);
    }
    let args = match parse(args) {
        Ok(a) => a,
        Err(msg) => {
            eprint!("{msg}\n\n{HELP}");
            return Ok(2);
        }
    };

    let releases = std::env::var("FRAPPE_RELEASES_URL").unwrap_or_else(|_| DEFAULT_RELEASES.to_string());
    let releases = releases.trim_end_matches('/');
    let current = format!("v{}", env!("CARGO_PKG_VERSION"));

    let target = match args.version {
        Some(v) => v,
        None => latest_tag(releases)?,
    };
    if args.check {
        if target == current {
            println!("{PROG} {current} is up to date");
        } else {
            println!("{PROG} {current} installed, {target} available — run \"{PROG} update\"");
        }
        return Ok(0);
    }
    if target == current {
        println!("{PROG} {current} is already installed");
        return Ok(0);
    }

    let exe = exe()?;
    let dir = exe
        .parent()
        .ok_or_else(|| CliError::new(format!("cannot find the directory of {}", exe.display())))?;
    let script = fetch_text(&format!("{releases}/download/{target}/install.sh"))?;
    let path = std::env::temp_dir().join(format!("frappe-install-{}.sh", std::process::id()));
    std::fs::write(&path, script)?;
    println!("Updating {PROG} {current} -> {target}");
    let status = Command::new("sh")
        .arg(&path)
        .arg(&target)
        .env("FRAPPE_INSTALL_DIR", dir)
        .env("FRAPPE_RELEASES_URL", releases)
        .status();
    let _ = std::fs::remove_file(&path);
    let status = status.map_err(|e| CliError::new(format!("cannot run sh: {e}")))?;
    if status.success() {
        Ok(0)
    } else {
        Err(CliError::new(format!(
            "update failed; {} was not changed unless the installer said otherwise",
            exe.display()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn tag_comes_from_the_redirect_target() {
        assert_eq!(tag_of("https://github.com/o/r/releases/tag/v2.0.1"), Some("v2.0.1"));
        assert_eq!(tag_of("https://github.com/o/r/releases/tag/v2.0.1?x=1"), Some("v2.0.1"));
        assert_eq!(tag_of("https://github.com/o/r/releases"), None);
        assert_eq!(tag_of("https://github.com/o/r/releases/tag/nightly"), None);
    }

    #[test]
    fn version_gets_a_v_prefix() {
        let a = parse(&argv(&["--check", "2.1.0"])).unwrap();
        assert!(a.check);
        assert_eq!(a.version.as_deref(), Some("v2.1.0"));
        assert_eq!(
            parse(&argv(&["v3.0.0-rc.1"])).unwrap().version.as_deref(),
            Some("v3.0.0-rc.1")
        );
    }

    #[test]
    fn rejects_unknown_options_and_extra_arguments() {
        assert!(parse(&argv(&["--nope"])).is_err());
        assert!(parse(&argv(&["v1.0.0", "v2.0.0"])).is_err());
    }
}
