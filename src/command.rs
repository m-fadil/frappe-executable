use crate::env::Env;
use crate::errors::Result;
use std::io::IsTerminal;

pub const PROG: &str = "frappe";

pub struct Command {
    /// subcommand name: `frappe <name>`
    pub name: &'static str,
    /// one line for the command list
    pub desc: &'static str,
    /// paragraph under the usage line
    pub about: &'static str,
    /// The variables it reads, with defaults; rendered by --help.
    /// A function so a command that wraps others can merge their lists.
    pub env_help: fn() -> String,
    pub run: fn(&mut Env) -> Result<()>,
}

pub fn help_text(cmd: &Command) -> String {
    format!(
        "Usage: {PROG} {} [options] [env-file...]

{}

Options:
  -h, --help              Show this help
  -e VAR=val, --env       Set/override a variable (repeatable; always wins)
      --env-file PATH     Load variables from an env file (repeatable)

Precedence: process environment, then --env-file (in order), then -e.

{}
",
        cmd.name,
        cmd.about,
        (cmd.env_help)()
    )
}

/// `[tag] message` logger, the prefix every site command prints with.
pub fn logger(tag: &'static str) -> impl Fn(&str) {
    move |message| println!("[{tag}] {message}")
}

#[derive(Clone, Copy)]
pub enum Color {
    Cyan,
    Yellow,
    Green,
}

/// Bold step/section line on stdout; plain when stdout is not a terminal.
pub fn heading(text: &str, color: Color) {
    if std::io::stdout().is_terminal() {
        let code = match color {
            Color::Cyan => 36,
            Color::Yellow => 33,
            Color::Green => 32,
        };
        println!("\x1b[1m\x1b[{code}m{text}\x1b[39m\x1b[22m");
    } else {
        println!("{text}");
    }
}
