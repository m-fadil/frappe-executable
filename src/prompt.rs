use crate::backups::path_choices;
use crate::env::Env;
use crate::errors::{CliError, Result};
use inquire::autocompletion::{Autocomplete, Replacement};
use inquire::validator::Validation;
use inquire::{Confirm, CustomUserError, Password, PasswordDisplayMode, Select, Text};
use std::io::IsTerminal;
use std::path::PathBuf;

/// Prompts are drawn on stderr, like `read -p` would, so stdout stays clean for
/// piping (`frappe create-user ... | tail -1`). Ctrl+C surfaces as an
/// InquireError, which the CLI turns into exit 130.
pub fn can_prompt() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

fn no_terminal(name: &str, extra: &str) -> CliError {
    CliError::new(format!(
        "{name} is required ({extra}no terminal to ask on; pass -e {name}=... or --env-file)"
    ))
}

pub fn prompt_text(message: &str, default: Option<&str>, required: bool) -> Result<String> {
    let mut p = Text::new(message);
    if let Some(d) = default.filter(|d| !d.is_empty()) {
        p = p.with_default(d);
    }
    if required {
        p = p.with_validator(|v: &str| {
            Ok(if v.trim().is_empty() {
                Validation::Invalid("Value is required".into())
            } else {
                Validation::Valid
            })
        });
    }
    Ok(p.prompt()?.trim().to_string())
}

pub fn prompt_secret(message: &str, allow_empty: bool) -> Result<String> {
    let mut p = Password::new(message)
        .with_display_mode(PasswordDisplayMode::Masked)
        .without_confirmation();
    if !allow_empty {
        p = p.with_validator(|v: &str| {
            Ok(if v.is_empty() {
                Validation::Invalid("Value is required".into())
            } else {
                Validation::Valid
            })
        });
    }
    Ok(p.prompt()?)
}

#[derive(Debug, Clone)]
pub struct Choice {
    pub value: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

impl Choice {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            name: None,
            description: None,
        }
    }

    pub fn named(value: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            name: Some(name.into()),
            description: None,
        }
    }

    pub fn described(value: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            name: None,
            description: Some(description.into()),
        }
    }

    fn label(&self) -> String {
        let base = self.name.as_deref().unwrap_or(&self.value);
        match &self.description {
            Some(d) => format!("{base} — {d}"),
            None => base.to_string(),
        }
    }
}

pub fn prompt_select(message: &str, choices: &[Choice], default: Option<&str>) -> Result<String> {
    let labels: Vec<String> = choices.iter().map(Choice::label).collect();
    let start = default
        .and_then(|d| choices.iter().position(|c| c.value == d))
        .unwrap_or(0);
    let picked = Select::new(message, labels)
        .with_page_size(20)
        .with_starting_cursor(start)
        .raw_prompt()?;
    Ok(choices[picked.index].value.clone())
}

pub fn prompt_confirm(message: &str, default: bool) -> Result<bool> {
    Ok(Confirm::new(message).with_default(default).prompt()?)
}

#[derive(Clone)]
struct PathCompleter {
    cwd: PathBuf,
}

impl Autocomplete for PathCompleter {
    fn get_suggestions(&mut self, input: &str) -> std::result::Result<Vec<String>, CustomUserError> {
        Ok(path_choices(input, &self.cwd).into_iter().map(|c| c.name).collect())
    }

    fn get_completion(
        &mut self,
        _input: &str,
        highlighted_suggestion: Option<String>,
    ) -> std::result::Result<Replacement, CustomUserError> {
        Ok(highlighted_suggestion)
    }
}

/// Free-text path input; Tab completes from the highlighted suggestion.
pub fn prompt_search(message: &str, validate: fn(&str) -> std::result::Result<(), String>) -> Result<String> {
    let cwd = std::env::current_dir()?;
    let p = Text::new(message)
        .with_autocomplete(PathCompleter { cwd })
        .with_page_size(15)
        .with_validator(move |v: &str| {
            Ok(match validate(v) {
                Ok(()) => Validation::Valid,
                Err(msg) => Validation::Invalid(msg.into()),
            })
        });
    Ok(p.prompt()?)
}

// --- "ask only when unset" fallbacks: -e / --env-file always win -----------

/// Ask for `name` unless set. Without a terminal, use `fallback` or fail naming the variable.
pub fn ask_var(env: &mut Env, name: &str, message: &str, fallback: &str) -> Result<String> {
    if env.filled(name) {
        return Ok(env.get(name).unwrap_or_default().to_string());
    }
    let mut value = fallback.to_string();
    if can_prompt() {
        value = prompt_text(message, Some(fallback), fallback.is_empty())?;
    } else if fallback.is_empty() {
        return Err(no_terminal(name, ""));
    }
    if value.is_empty() {
        return Err(CliError::new(format!("{name} is required")));
    }
    env.set(name, &value);
    Ok(value)
}

/// Ask for a secret unless set: masked, never defaulted.
pub fn ask_secret(env: &mut Env, name: &str, message: &str) -> Result<String> {
    if env.filled(name) {
        return Ok(env.get(name).unwrap_or_default().to_string());
    }
    if !can_prompt() {
        return Err(no_terminal(name, ""));
    }
    let value = prompt_secret(message, false)?;
    env.set(name, &value);
    Ok(value)
}

/// Arrow-key pick unless set.
pub fn ask_select(env: &mut Env, name: &str, message: &str, choices: &[Choice], extra: &str) -> Result<String> {
    if env.filled(name) {
        return Ok(env.get(name).unwrap_or_default().to_string());
    }
    if !can_prompt() {
        return Err(no_terminal(name, extra));
    }
    let value = prompt_select(message, choices, None)?;
    env.set(name, &value);
    Ok(value)
}
