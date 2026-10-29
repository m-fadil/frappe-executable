use crate::errors::{CliError, Result};
use std::collections::HashMap;

/// Parse a dotenv-style file with the shell rules people actually write:
///
///   # comment                    KEY=value          export KEY=value
///   KEY='literal $not expanded'  KEY="a $B ${C} \" \\ \$"
///   KEY=value   # trailing comment (needs whitespace before '#')
///
/// `$NAME` / `${NAME}` expand from `lookup` (earlier lines, earlier files, the
/// process environment). An unquoted value containing whitespace is an error
/// rather than being silently cut, since the shell would run it as a command.
pub fn parse_env_file(
    text: &str,
    file: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen: HashMap<String, String> = HashMap::new();

    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let mut idx = 0;
    while idx < lines.len() {
        let where_ = format!("{file}:{}", idx + 1);
        let raw = lines[idx];
        let mut line = raw.trim_start();
        if line.is_empty() || line.starts_with('#') {
            idx += 1;
            continue;
        }
        let mut exported = false;
        if let Some(rest) = line.strip_prefix("export")
            && rest.starts_with(char::is_whitespace)
        {
            exported = true;
            line = rest.trim_start();
        }
        // `export NAME` alone only marks a variable exported: nothing to load
        if exported && is_bare_name(line) {
            idx += 1;
            continue;
        }

        let name_len = name_prefix_len(line);
        if name_len == 0 || !line[name_len..].starts_with('=') {
            return Err(CliError::new(format!(
                "{where_}: expected KEY=value, got: {}",
                raw.trim()
            )));
        }
        let key = line[..name_len].to_string();
        let mut src = line[name_len + 1..].to_string();
        // a quoted value may span lines, as it would when sourced by a shell
        let value = loop {
            let get = |n: &str| seen.get(n).cloned().or_else(|| lookup(n));
            match parse_value(&src, &where_, &get) {
                Ok(v) => break v,
                Err(ValueError::Unterminated(e)) => {
                    if idx + 1 >= lines.len() {
                        return Err(e);
                    }
                    idx += 1;
                    src.push('\n');
                    src.push_str(lines[idx]);
                }
                Err(ValueError::Fatal(e)) => return Err(e),
            }
        };
        seen.insert(key.clone(), value.clone());
        out.push((key, value));
        idx += 1;
    }
    Ok(out)
}

/// `NAME` optionally followed by whitespace and a `# comment`.
fn is_bare_name(line: &str) -> bool {
    let n = name_prefix_len(line);
    if n == 0 {
        return false;
    }
    let rest = line[n..].trim_start();
    rest.is_empty() || rest.starts_with('#')
}

/// Byte length of the leading `[A-Za-z_][A-Za-z0-9_]*`, 0 if none.
fn name_prefix_len(s: &str) -> usize {
    let mut n = 0;
    for (i, c) in s.char_indices() {
        let ok = if i == 0 {
            c.is_ascii_alphabetic() || c == '_'
        } else {
            c.is_ascii_alphanumeric() || c == '_'
        };
        if !ok {
            break;
        }
        n = i + 1;
    }
    n
}

enum ValueError {
    /// Mid-value: the caller may append the next line and try again.
    Unterminated(CliError),
    Fatal(CliError),
}

fn parse_value(
    src: &str,
    where_: &str,
    get: &dyn Fn(&str) -> Option<String>,
) -> std::result::Result<String, ValueError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::new();
    let mut i = 0;

    // `$NAME` or `${NAME}` starting at chars[i] == '$'; returns (expansion, chars consumed)
    let expand = |i: usize| -> std::result::Result<(String, usize), ValueError> {
        let rest = &chars[i + 1..];
        if rest.first() == Some(&'{') {
            let end = rest.iter().position(|&c| c == '}');
            let name: String = match end {
                Some(e) => rest[1..e].iter().collect(),
                None => String::new(),
            };
            let valid = !name.is_empty() && name_prefix_len(&name) == name.len();
            if !valid {
                let shown: String = match end {
                    Some(_) => name,
                    None => rest[1..].iter().collect(),
                };
                return Err(ValueError::Fatal(CliError::new(format!(
                    "{where_}: unsupported expansion ${{{shown}}}"
                ))));
            }
            return Ok((get(&name).unwrap_or_default(), end.unwrap() + 2));
        }
        let rest_s: String = rest.iter().collect();
        let n = name_prefix_len(&rest_s);
        if n == 0 {
            return Ok(("$".to_string(), 1));
        }
        Ok((get(&rest_s[..n]).unwrap_or_default(), rest_s[..n].chars().count() + 1))
    };

    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            let Some(end) = chars[i + 1..].iter().position(|&x| x == '\'') else {
                return Err(ValueError::Unterminated(CliError::new(format!(
                    "{where_}: unterminated single quote"
                ))));
            };
            out.extend(&chars[i + 1..i + 1 + end]);
            i += end + 2;
        } else if c == '"' {
            i += 1;
            loop {
                if i >= chars.len() {
                    return Err(ValueError::Unterminated(CliError::new(format!(
                        "{where_}: unterminated double quote"
                    ))));
                }
                let d = chars[i];
                if d == '"' {
                    i += 1;
                    break;
                }
                if d == '\\' && i + 1 < chars.len() && matches!(chars[i + 1], '"' | '\\' | '$' | '`') {
                    out.push(chars[i + 1]);
                    i += 2;
                } else if d == '$' {
                    let (v, n) = expand(i)?;
                    out.push_str(&v);
                    i += n;
                } else {
                    out.push(d);
                    i += 1;
                }
            }
        } else if c == ' ' || c == '\t' {
            let rest: String = chars[i..].iter().collect();
            let rest = rest.trim_start();
            if !rest.is_empty() && !rest.starts_with('#') {
                return Err(ValueError::Fatal(CliError::new(format!(
                    "{where_}: unquoted whitespace in value — wrap the value in quotes"
                ))));
            }
            break;
        } else if c == '\\' && i + 1 < chars.len() {
            out.push(chars[i + 1]);
            i += 2;
        } else if c == '$' {
            let (v, n) = expand(i)?;
            out.push_str(&v);
            i += n;
        } else {
            out.push(c);
            i += 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str, vars: &[(&str, &str)]) -> Result<HashMap<String, String>> {
        let lookup = |n: &str| vars.iter().find(|(k, _)| *k == n).map(|(_, v)| v.to_string());
        Ok(parse_env_file(text, "x.env", &lookup)?.into_iter().collect())
    }

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn err(text: &str) -> String {
        parse(text, &[]).unwrap_err().message
    }

    #[test]
    fn comments_blank_lines_export_prefix_trailing_comments() {
        let got = parse("# c\n\nexport A=1\nB=2   # note\nC=   # empty\nD=a#b\n", &[]).unwrap();
        assert_eq!(got, map(&[("A", "1"), ("B", "2"), ("C", ""), ("D", "a#b")]));
    }

    #[test]
    fn quoting() {
        let got = parse(
            "A='lit $X \"q\"'\nB=\"dq \\\" \\\\ \\$X\"\nC=\"a b\"'c d'\nD=\"\"",
            &[("X", "x")],
        )
        .unwrap();
        assert_eq!(
            got,
            map(&[("A", "lit $X \"q\""), ("B", "dq \" \\ $X"), ("C", "a bc d"), ("D", "")])
        );
    }

    #[test]
    fn expansion_from_earlier_lines_then_lookup() {
        let got = parse(
            "A=1\nB=$A-${A}\nC=\"$OUTER/$NOPE\"\nD=$\n",
            &[("OUTER", "o"), ("A", "ignored")],
        )
        .unwrap();
        assert_eq!(got, map(&[("A", "1"), ("B", "1-1"), ("C", "o/"), ("D", "$")]));
    }

    #[test]
    fn crlf() {
        assert_eq!(
            parse("A=1\r\nB=\"2\"\r\n", &[]).unwrap(),
            map(&[("A", "1"), ("B", "2")])
        );
    }

    #[test]
    fn quoted_values_span_lines_and_line_numbers_stay_right() {
        let got = parse("A=\"one\ntwo\"\nB='x\n# not a comment'\nC=3\n", &[]).unwrap();
        assert_eq!(got, map(&[("A", "one\ntwo"), ("B", "x\n# not a comment"), ("C", "3")]));
        assert!(err("A=\"one\ntwo\"\nB=bad value\n").contains("x.env:3: unquoted whitespace"));
    }

    #[test]
    fn bom_and_bare_export() {
        assert_eq!(
            parse("\u{FEFF}A=1\nexport A\nexport B # marks only\n", &[]).unwrap(),
            map(&[("A", "1")])
        );
    }

    #[test]
    fn errors_name_file_and_line() {
        assert!(err("A=1\nB=two words\n").contains("x.env:2: unquoted whitespace"));
        assert!(err("A='open\n").contains("x.env:1: unterminated single quote"));
        assert!(err("A=\"open\n").contains("unterminated double quote"));
        assert!(err("not an assignment\n").contains("x.env:1: expected KEY=value"));
        assert!(err("A=${B:-x}\n").contains("unsupported expansion"));
    }

    #[test]
    fn env_example_parses_with_nothing_set() {
        let got = parse(include_str!("../.env.example"), &[]).unwrap();
        assert_eq!(got["MODE"], "docker");
        assert_eq!(got["DB_ROOT_PASS"], "");
        assert_eq!(got["RESTORE_SOURCE_TOKEN"], "token abcdef123456:secret789");
    }
}
