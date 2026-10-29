use crate::envfile::parse_env_file;
use crate::errors::{CliError, Result, UsageError};
use std::collections::HashMap;
use std::fs;

/// The variables a command runs with: the process environment, then each env
/// file in order (later wins), then every `-e` override (always wins).
/// Child processes get the merged result, just as exported shell vars would.
#[derive(Debug, Default, Clone)]
pub struct Env {
    vars: HashMap<String, String>,
}

impl Env {
    pub fn from_process() -> Self {
        let vars = std::env::vars_os()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned()))
            .collect();
        Self { vars }
    }

    /// Raw value; `None` only when unset (an empty string is a value).
    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }

    /// Value, or `fallback` when unset OR empty (shell `${NAME:-fallback}`).
    pub fn or(&self, name: &str, fallback: &str) -> String {
        match self.get(name) {
            Some(v) if !v.is_empty() => v.to_string(),
            _ => fallback.to_string(),
        }
    }

    /// True when set to a non-empty value.
    pub fn filled(&self, name: &str) -> bool {
        self.get(name).is_some_and(|v| !v.is_empty())
    }

    /// Non-empty value or a CliError naming the variable.
    pub fn need(&self, name: &str) -> Result<String> {
        match self.get(name) {
            Some(v) if !v.is_empty() => Ok(v.to_string()),
            _ => Err(CliError::new(format!("{name} is required"))),
        }
    }

    pub fn set(&mut self, name: &str, value: &str) {
        self.vars.insert(name.to_string(), value.to_string());
    }

    pub fn vars(&self) -> &HashMap<String, String> {
        &self.vars
    }
}

#[derive(Debug, Default)]
pub struct ParsedArgs {
    pub help: bool,
    pub env_files: Vec<String>,
    pub overrides: Vec<(String, String)>,
}

fn split_override(kv: &str) -> std::result::Result<(String, String), UsageError> {
    let bytes = kv.as_bytes();
    let valid_name = |b: &[u8]| {
        !b.is_empty()
            && (b[0].is_ascii_alphabetic() || b[0] == b'_')
            && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
    };
    match kv.find('=') {
        Some(eq) if valid_name(&bytes[..eq]) => Ok((kv[..eq].to_string(), kv[eq + 1..].to_string())),
        _ => Err(UsageError(format!("invalid override: {kv} (expected VAR=val)"))),
    }
}

/// Options every command accepts:
///   -h | --help
///   -e VAR=val | -eVAR=val | --env VAR=val | --env=VAR=val   (repeatable)
///   --env-file PATH | --env-file=PATH                        (repeatable)
///   PATH                                                     (bare positional = env file)
///   --                                                       (everything after is an env file)
/// Stops at the first -h/--help, so `-h` works even next to a bad option.
pub fn parse_args(argv: &[String]) -> std::result::Result<ParsedArgs, UsageError> {
    let mut res = ParsedArgs::default();
    let mut i = 0;
    while i < argv.len() {
        let a = argv[i].as_str();
        let mut next = |what: &str| -> std::result::Result<String, UsageError> {
            if i + 1 >= argv.len() {
                return Err(UsageError(format!("{a} requires {what}")));
            }
            i += 1;
            Ok(argv[i].clone())
        };
        if a == "-h" || a == "--help" {
            res.help = true;
            return Ok(res);
        } else if a == "-e" || a == "--env" {
            res.overrides.push(split_override(&next("VAR=val")?)?);
        } else if let Some(kv) = a.strip_prefix("--env=") {
            res.overrides.push(split_override(kv)?);
        } else if let Some(kv) = a.strip_prefix("-e") {
            res.overrides.push(split_override(kv)?);
        } else if a == "--env-file" {
            res.env_files.push(next("a path")?);
        } else if let Some(p) = a.strip_prefix("--env-file=") {
            res.env_files.push(p.to_string());
        } else if a == "--" {
            res.env_files.extend(argv[i + 1..].iter().cloned());
            i = argv.len();
            continue;
        } else if a.starts_with('-') {
            return Err(UsageError(format!("unknown option: {a}")));
        } else {
            res.env_files.push(a.to_string());
        }
        i += 1;
    }
    Ok(res)
}

pub fn load_env(args: &ParsedArgs, base: Env) -> Result<Env> {
    let mut env = base;
    for file in &args.env_files {
        if !fs::metadata(file).map(|m| m.is_file()).unwrap_or(false) {
            return Err(CliError::new(format!("env file not found: {file}")));
        }
        let bytes = fs::read(file).map_err(|e| CliError::new(format!("cannot read env file: {file}: {e}")))?;
        let text = String::from_utf8_lossy(&bytes);
        let pairs = parse_env_file(&text, file, &|n| env.get(n).map(String::from))?;
        for (k, v) in pairs {
            env.set(&k, &v);
        }
    }
    for (k, v) in &args.overrides {
        env.set(k, v);
    }
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn argv(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    fn write(dir: &Path, name: &str, text: &str) -> String {
        let p = dir.join(name);
        fs::write(&p, text).unwrap();
        p.to_string_lossy().into_owned()
    }

    fn load(a: &[&str]) -> Env {
        load_env(&parse_args(&argv(a)).unwrap(), Env::default()).unwrap()
    }

    fn usage_err(a: &[&str]) -> String {
        parse_args(&argv(a)).unwrap_err().0
    }

    #[test]
    fn bare_positional_and_env_file_forms_load_a_file() {
        let d = tempfile::tempdir().unwrap();
        let one = write(d.path(), "one.env", "A=from_file\nB=b_file\n");
        for a in [
            vec![one.clone()],
            vec!["--env-file".into(), one.clone()],
            vec![format!("--env-file={one}")],
        ] {
            let a: Vec<&str> = a.iter().map(String::as_str).collect();
            let env = load(&a);
            assert_eq!(env.get("A"), Some("from_file"));
            assert_eq!(env.get("B"), Some("b_file"));
        }
    }

    #[test]
    fn precedence() {
        let d = tempfile::tempdir().unwrap();
        let one = write(d.path(), "one.env", "A=from_file\nB=b_file\n");
        let two = write(d.path(), "two.env", "A=second_file\n");
        assert_eq!(load(&[&one, &two]).get("A"), Some("second_file"));
        let env = load(&["-e", "A=cli", &one, &two]);
        assert_eq!(env.get("A"), Some("cli"));
        assert_eq!(env.get("B"), Some("b_file"));

        let mut base = Env::default();
        base.set("A", "process");
        base.set("Z", "z");
        let env = load_env(&parse_args(&argv(&[&one])).unwrap(), base).unwrap();
        assert_eq!(env.get("A"), Some("from_file"));
        assert_eq!(env.get("Z"), Some("z"));
    }

    #[test]
    fn override_spellings_value_keeps_equals_and_spaces() {
        let env = load(&["-eA=1", "--env", "B=2", "--env=C=a=b c", "-e", "D="]);
        let got: Vec<_> = ["A", "B", "C", "D"].iter().map(|k| env.get(k)).collect();
        assert_eq!(got, [Some("1"), Some("2"), Some("a=b c"), Some("")]);
    }

    #[test]
    fn double_dash_turns_rest_into_env_files() {
        assert_eq!(
            parse_args(&argv(&["--", "-weird.env"])).unwrap().env_files,
            ["-weird.env"]
        );
    }

    #[test]
    fn help_stops_parsing() {
        assert!(parse_args(&argv(&["-h", "--nope"])).unwrap().help);
        assert!(parse_args(&argv(&["--help"])).unwrap().help);
    }

    #[test]
    fn usage_errors() {
        assert_eq!(usage_err(&["--nope"]), "unknown option: --nope");
        assert_eq!(usage_err(&["-"]), "unknown option: -");
        assert!(usage_err(&["-e", "notanassignment"]).starts_with("invalid override: notanassignment"));
        assert!(usage_err(&["-e", "1A=x"]).starts_with("invalid override"));
        assert_eq!(usage_err(&["-e"]), "-e requires VAR=val");
        assert_eq!(usage_err(&["--env-file"]), "--env-file requires a path");
    }

    #[test]
    fn missing_env_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("absent.env").to_string_lossy().into_owned();
        let e = load_env(&parse_args(&argv(&[&p])).unwrap(), Env::default()).unwrap_err();
        assert!(e.message.starts_with("env file not found"));
    }

    #[test]
    fn unset_vs_empty() {
        let env = load(&["-e", "EMPTY="]);
        assert_eq!(env.get("EMPTY"), Some(""));
        assert_eq!(env.get("UNSET"), None);
        assert_eq!(env.or("EMPTY", "d"), "d");
        assert_eq!(env.or("UNSET", "d"), "d");
    }
}
