use chrono::{DateTime, Local, NaiveDate};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// What `bench restore` takes as a database backup.
pub fn is_backup_name(name: &str) -> bool {
    name.ends_with(".sql") || name.ends_with(".sql.gz")
}

#[derive(Debug, Clone)]
pub struct Backup {
    pub path: PathBuf,
    pub name: String,
    /// From the `YYYYMMDD_HHMMSS-` prefix bench writes, else the file's mtime.
    pub time: DateTime<Local>,
    pub size: u64,
}

/// Resolve `.` / `..` / duplicate slashes without touching the filesystem.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() && !out.has_root() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

/// `~` and `~/x` as a shell would expand them, then made absolute.
pub fn expand_path(path: &str, cwd: &Path) -> PathBuf {
    if path == "~" || path.starts_with("~/") {
        let home = std::env::home_dir().unwrap_or_default();
        return normalize(&home.join(path[1..].trim_start_matches('/')));
    }
    normalize(&cwd.join(path))
}

struct Entry {
    name: String,
    meta: fs::Metadata,
}

/// Entries of `dir` with symlinks followed; unreadable dirs and dangling links are skipped.
fn entries(dir: &Path) -> Vec<Entry> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    rd.flatten()
        .filter_map(|e| {
            let meta = fs::metadata(e.path()).ok()?;
            Some(Entry {
                name: e.file_name().to_string_lossy().into_owned(),
                meta,
            })
        })
        .collect()
}

/// The timestamp bench puts in front of backup names, read as local time.
pub fn name_time(name: &str) -> Option<DateTime<Local>> {
    let b = name.as_bytes();
    if b.len() < 15 || b[8] != b'_' {
        return None;
    }
    let digits = |r: std::ops::Range<usize>| -> Option<u32> {
        let s = &b[r];
        if s.iter().all(u8::is_ascii_digit) {
            std::str::from_utf8(s).ok()?.parse().ok()
        } else {
            None
        }
    };
    let (y, mo, d) = (digits(0..4)?, digits(4..6)?, digits(6..8)?);
    let (h, mi, s) = (digits(9..11)?, digits(11..13)?, digits(13..15)?);
    let naive = NaiveDate::from_ymd_opt(y as i32, mo, d)?.and_hms_opt(h, mi, s)?;
    naive.and_local_timezone(Local).earliest()
}

/// Database backups directly inside `dir`, newest first.
pub fn list_backups(dir: &Path) -> Vec<Backup> {
    let mut out: Vec<Backup> = entries(dir)
        .into_iter()
        .filter(|e| e.meta.is_file() && is_backup_name(&e.name))
        .map(|e| {
            let time = name_time(&e.name).or_else(|| e.meta.modified().ok().map(DateTime::<Local>::from));
            Backup {
                path: dir.join(&e.name),
                time: time.unwrap_or_default(),
                size: e.meta.len(),
                name: e.name,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.time
            .timestamp_millis()
            .cmp(&a.time.timestamp_millis())
            .then_with(|| b.name.cmp(&a.name))
    });
    out
}

#[derive(Debug, Clone)]
pub struct PathChoice {
    /// Absolute path.
    pub value: PathBuf,
    /// Exactly what Tab writes into the input.
    pub name: String,
}

/// Completions for a typed path: the path itself when it is usable, then the
/// directories and backups in its directory whose name starts with what follows
/// the last `/`. Hidden entries only show once the typed part starts with `.`.
pub fn path_choices(term: &str, cwd: &Path) -> Vec<PathChoice> {
    let slash = term.rfind('/').map_or(0, |i| i + 1);
    let (dir_part, base) = term.split_at(slash);
    let dir = expand_path(if dir_part.is_empty() { "." } else { dir_part }, cwd);

    let mut found: Vec<Entry> = entries(&dir)
        .into_iter()
        .filter(|e| e.name.starts_with(base) && (base.starts_with('.') || !e.name.starts_with('.')))
        .filter(|e| e.meta.is_dir() || (e.meta.is_file() && is_backup_name(&e.name)))
        .collect();
    found.sort_by(|a, b| b.meta.is_dir().cmp(&a.meta.is_dir()).then_with(|| a.name.cmp(&b.name)));
    let listed: Vec<PathChoice> = found
        .iter()
        .map(|e| PathChoice {
            value: dir.join(&e.name),
            name: format!("{dir_part}{}{}", e.name, if e.meta.is_dir() { "/" } else { "" }),
        })
        .collect();

    let self_path = expand_path(if term.is_empty() { "." } else { term }, cwd);
    let usable = fs::metadata(&self_path)
        .map(|m| m.is_dir() || (m.is_file() && is_backup_name(&self_path.to_string_lossy())))
        .unwrap_or(false);
    if !usable || listed.iter().any(|c| c.value == self_path) {
        return listed;
    }
    let own = if term.is_empty() {
        PathChoice {
            value: self_path,
            name: "./".into(),
        }
    } else {
        PathChoice {
            value: self_path,
            name: term.to_string(),
        }
    };
    let mut all = vec![own];
    all.extend(listed);
    all
}

/// Why `path` cannot be restored from, or Ok.
pub fn check_backup_path(path: &Path) -> Result<(), String> {
    let Ok(meta) = fs::metadata(path) else {
        return Err(format!("{} does not exist", path.display()));
    };
    if meta.is_dir() {
        return if list_backups(path).is_empty() {
            Err(format!("No .sql / .sql.gz backups in {}", path.display()))
        } else {
            Ok(())
        };
    }
    if is_backup_name(&path.to_string_lossy()) {
        Ok(())
    } else {
        Err(format!("{} is not a .sql or .sql.gz backup", path.display()))
    }
}

pub fn format_backup(b: &Backup) -> String {
    let kb = b.size as f64 / 1024.0;
    let size = if kb < 1024.0 {
        format!("{kb:.1} KB")
    } else if kb < 1024.0 * 1024.0 {
        format!("{:.1} MB", kb / 1024.0)
    } else {
        format!("{:.1} GB", kb / 1024.0 / 1024.0)
    };
    format!("{}  {}  ({size})", b.time.format("%Y-%m-%d %H:%M"), b.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, create_dir};
    use std::os::unix::fs::symlink;
    use std::time::{Duration, SystemTime};

    /// Files with explicit mtimes (seconds), so ordering never depends on the clock.
    fn tree(root: &Path, name: &str, files: &[(&str, u64)]) -> PathBuf {
        let dir = root.join(name);
        create_dir(&dir).unwrap();
        for (file, mtime) in files {
            fs::write(dir.join(file), "x").unwrap();
            let f = File::options().write(true).open(dir.join(file)).unwrap();
            f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(*mtime))
                .unwrap();
        }
        dir
    }

    #[test]
    fn list_backups_newest_first_by_name_timestamp_then_mtime() {
        let root = tempfile::tempdir().unwrap();
        let dir = tree(
            root.path(),
            "list",
            &[
                // copied without -p: the oldest backup has the newest mtime
                ("20240101_010000-a-database.sql.gz", 1_900_000_000),
                ("20250601_120000-a-database.sql.gz", 1_000_000_000),
                // no timestamp in the name: 2025-11-14 by mtime
                ("manual.sql", 1_763_100_000),
                // not real times: fall back to mtime (2001), ties broken by name
                ("20251399_000000-a-database.sql.gz", 1_000_000_000),
                ("20250101_007000-a-database.sql.gz", 1_000_000_000),
                ("20250601_120000-a-files.tar", 1_000_000_000),
                ("site_config_backup.json", 1_000_000_000),
            ],
        );
        let names: Vec<_> = list_backups(&dir).into_iter().map(|b| b.name).collect();
        assert_eq!(
            names,
            [
                "manual.sql",
                "20250601_120000-a-database.sql.gz",
                "20240101_010000-a-database.sql.gz",
                "20251399_000000-a-database.sql.gz",
                "20250101_007000-a-database.sql.gz",
            ]
        );
    }

    #[test]
    fn missing_directory_has_no_backups() {
        let root = tempfile::tempdir().unwrap();
        assert!(list_backups(&root.path().join("nope")).is_empty());
    }

    fn pick_tree(root: &Path) -> PathBuf {
        let dir = tree(root, "pick", &[("b.sql.gz", 1), ("notes.txt", 1), (".hidden.sql", 1)]);
        create_dir(dir.join("archive")).unwrap();
        create_dir(dir.join(".cache")).unwrap();
        symlink(dir.join("archive"), dir.join("linked")).unwrap();
        symlink(dir.join("gone"), dir.join("dangling.sql")).unwrap();
        dir
    }

    fn names(term: &str, dir: &Path) -> Vec<String> {
        path_choices(term, dir).into_iter().map(|c| c.name).collect()
    }

    #[test]
    fn empty_input_offers_current_directory_then_dirs_before_backups() {
        let root = tempfile::tempdir().unwrap();
        let dir = pick_tree(root.path());
        assert_eq!(names("", &dir), ["./", "archive/", "linked/", "b.sql.gz"]);
        assert_eq!(path_choices("", &dir)[0].value, dir);
    }

    #[test]
    fn completes_after_last_slash_hidden_only_when_typed() {
        let root = tempfile::tempdir().unwrap();
        let dir = pick_tree(root.path());
        assert_eq!(names("a", &dir), ["archive/"]);
        assert_eq!(names(".", &dir), [".", ".cache/", ".hidden.sql"]);
        assert_eq!(names("archive/", &dir), ["archive/"]);
        // the typed dir and its listed completion are one entry, not two
        assert_eq!(names("archive", &dir), ["archive/"]);
        assert_eq!(names("./archive", &dir), ["./archive/"]);
        assert_eq!(
            names(&format!("{}/b", dir.display()), &dir),
            [format!("{}/b.sql.gz", dir.display())]
        );
    }

    #[test]
    fn unusable_typed_path_offers_only_completions() {
        let root = tempfile::tempdir().unwrap();
        let dir = pick_tree(root.path());
        assert!(names("notes.txt", &dir).is_empty());
        assert!(names("zzz/", &dir).is_empty());
    }

    #[test]
    fn check_backup_path_accepts_and_names_problems() {
        let root = tempfile::tempdir().unwrap();
        let dir = tree(root.path(), "check", &[("x.sql.gz", 1), ("x.tar", 1)]);
        let empty = tree(root.path(), "check-empty", &[("x.tar", 1)]);
        assert!(check_backup_path(&dir).is_ok());
        assert!(check_backup_path(&dir.join("x.sql.gz")).is_ok());
        assert!(
            check_backup_path(&empty)
                .unwrap_err()
                .contains("No .sql / .sql.gz backups in")
        );
        assert!(
            check_backup_path(&dir.join("x.tar"))
                .unwrap_err()
                .contains("is not a .sql or .sql.gz backup")
        );
        assert!(
            check_backup_path(&dir.join("missing"))
                .unwrap_err()
                .contains("does not exist")
        );
    }
}
