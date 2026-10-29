use crate::errors::{CliError, Result};
use flate2::read::MultiGzDecoder;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// True when the whole file decompresses cleanly (`gzip -t`).
pub fn gzip_intact(path: &Path) -> bool {
    let Ok(file) = File::open(path) else { return false };
    io::copy(&mut MultiGzDecoder::new(io::BufReader::new(file)), &mut io::sink()).is_ok()
}

pub struct DownloadOptions<'a> {
    pub headers: Vec<(String, String)>,
    pub log: &'a dyn Fn(&str),
    /// total tries, counting the first; network errors, 408/429 and 5xx are retried (default 4)
    pub attempts: u32,
    /// first retry delay, doubling after each failure (default 1000)
    pub backoff_ms: u64,
    /// progress line on stderr
    pub progress: bool,
}

/// HTTP client where 4xx/5xx arrive as responses (redirects are followed).
pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().http_status_as_error(false).build().into()
}

const MIB: f64 = 1024.0 * 1024.0;

enum Outcome {
    Done,
    /// Partial discarded or untrusted: go again without counting a failure.
    Restart,
}

struct Fail {
    message: String,
    retryable: bool,
}

impl Fail {
    fn io(e: impl std::fmt::Display) -> Self {
        Self {
            message: format!("download failed: {e}"),
            retryable: true,
        }
    }
}

fn header<'a>(resp: &'a ureq::http::Response<ureq::Body>, name: &str) -> &'a str {
    resp.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or("")
}

/// Download `url` to `dest` through `dest.part`, renamed only on success, so an
/// interrupted run resumes where it stopped (HTTP Range) instead of starting
/// over, and a half-written `dest` never exists. A source that ignores Range is
/// re-downloaded from zero. On final failure the partial is kept for next time.
pub fn download_resumable(url: &str, dest: &Path, opts: &DownloadOptions) -> Result<()> {
    let mut part_os = dest.as_os_str().to_owned();
    part_os.push(".part");
    let part = PathBuf::from(part_os);
    let agent = agent();

    // only failures count against `attempts`; restarting from zero does not
    let mut failures = 0u32;
    loop {
        let have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if have > 0 {
            (opts.log)(&format!("resuming from {have} bytes"));
        }
        match attempt(&agent, url, dest, &part, have, opts) {
            Ok(Outcome::Done) => return Ok(()),
            Ok(Outcome::Restart) => continue,
            Err(fail) => {
                failures += 1;
                if !fail.retryable || failures >= opts.attempts {
                    if part.exists() {
                        (opts.log)(&format!("partial kept at {}, rerun to resume", part.display()));
                    }
                    return Err(CliError::new(fail.message));
                }
                let wait = opts.backoff_ms * 2u64.pow(failures - 1);
                (opts.log)(&format!(
                    "{} — retrying in {}s ({failures}/{})",
                    fail.message,
                    wait as f64 / 1000.0,
                    opts.attempts - 1
                ));
                std::thread::sleep(Duration::from_millis(wait));
            }
        }
    }
}

fn attempt(
    agent: &ureq::Agent,
    url: &str,
    dest: &Path,
    part: &Path,
    have: u64,
    opts: &DownloadOptions,
) -> std::result::Result<Outcome, Fail> {
    let mut req = agent.get(url);
    for (k, v) in &opts.headers {
        req = req.header(k.as_str(), v.as_str());
    }
    if have > 0 {
        req = req.header("Range", format!("bytes={have}-"));
    }
    let resp = req.call().map_err(Fail::io)?;
    let status = resp.status().as_u16();

    let mut append = false;
    if status == 206 && have > 0 {
        // only trust the resume when it starts exactly where our partial ends
        let start = header(&resp, "content-range")
            .strip_prefix("bytes ")
            .and_then(|r| r.split('-').next())
            .and_then(|s| s.parse::<u64>().ok());
        if start != Some(have) {
            drop(resp);
            (opts.log)("source answered with a different range, restarting the download");
            let _ = fs::remove_file(part);
            return Ok(Outcome::Restart);
        }
        append = true;
    } else if status == 416 && have > 0 {
        // killed between the last byte and the rename: the partial is already whole
        let total = header(&resp, "content-range")
            .strip_prefix("bytes */")
            .and_then(|s| s.parse::<u64>().ok());
        drop(resp);
        if total == Some(have) {
            fs::rename(part, dest).map_err(Fail::io)?;
            return Ok(Outcome::Done);
        }
        (opts.log)("source rejected the resume range, restarting the download");
        let _ = fs::remove_file(part);
        return Ok(Outcome::Restart);
    } else if !(200..300).contains(&status) {
        // the statuses curl --retry treats as transient
        let retryable = matches!(status, 408 | 429 | 500 | 502 | 503 | 504);
        let reason = resp.status().canonical_reason().unwrap_or("");
        let sep = if reason.is_empty() { "" } else { " " };
        let message = format!("download failed: HTTP {status}{sep}{reason} for {url}");
        return Err(Fail { message, retryable });
    } else if status == 200 && have > 0 {
        (opts.log)("source does not support resume, restarting the download");
    }

    let length: u64 = header(&resp, "content-length").parse().unwrap_or(0);
    let total = if length > 0 {
        length + if append { have } else { 0 }
    } else {
        0
    };
    let mut meter = Meter::new(if append { have } else { 0 }, total, opts.progress);
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .append(append)
        .truncate(!append)
        .open(part)
        .map_err(Fail::io)?;
    let mut reader = resp.into_body().into_reader();
    let mut buf = vec![0u8; 64 * 1024];
    let copied = loop {
        match reader.read(&mut buf) {
            Ok(0) => break Ok(()),
            Ok(n) => {
                if let Err(e) = file.write_all(&buf[..n]) {
                    break Err(e);
                }
                meter.add(n as u64);
            }
            Err(e) => break Err(e),
        }
    };
    meter.done();
    copied.map_err(Fail::io)?;
    drop(file);
    fs::rename(part, dest).map_err(Fail::io)?;
    Ok(Outcome::Done)
}

struct Meter {
    seen: u64,
    total: u64,
    enabled: bool,
    last: Option<Instant>,
}

impl Meter {
    fn new(start: u64, total: u64, enabled: bool) -> Self {
        Self {
            seen: start,
            total,
            enabled,
            last: None,
        }
    }

    fn add(&mut self, n: u64) {
        self.seen += n;
        if self.enabled && self.last.is_none_or(|t| t.elapsed() > Duration::from_millis(200)) {
            self.last = Some(Instant::now());
            self.draw();
        }
    }

    fn draw(&self) {
        let done = self.seen as f64 / MIB;
        let line = if self.total > 0 {
            let pct = (self.seen as f64 / self.total as f64 * 100.0).floor();
            format!("{done:.1} / {:.1} MiB ({pct}%)", self.total as f64 / MIB)
        } else {
            format!("{done:.1} MiB")
        };
        eprint!("\r  {line}   ");
    }

    fn done(&self) {
        if self.enabled {
            self.draw();
            eprintln!();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};
    use std::io::{BufRead, BufReader};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    fn body() -> Vec<u8> {
        "0123456789".repeat(1000).into_bytes()
    }

    /// (url, Range header) per request
    type Seen = Arc<Mutex<Vec<(String, Option<String>)>>>;

    struct Server {
        base: String,
        /// (url, Range header) per request
        seen: Seen,
        flaky_drops: Arc<AtomicUsize>,
    }

    fn serve() -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let flaky_drops = Arc::new(AtomicUsize::new(0));
        let (s, f) = (seen.clone(), flaky_drops.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (s, f) = (s.clone(), f.clone());
                std::thread::spawn(move || handle(stream, s, f));
            }
        });
        Server {
            base,
            seen,
            flaky_drops,
        }
    }

    fn handle(mut stream: TcpStream, seen: Seen, drops: Arc<AtomicUsize>) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        let url = request_line.split_whitespace().nth(1).unwrap_or("").to_string();
        let (mut range, mut auth) = (None, None);
        loop {
            let mut l = String::new();
            reader.read_line(&mut l).unwrap();
            let l = l.trim_end();
            if l.is_empty() {
                break;
            }
            if let Some((k, v)) = l.split_once(':') {
                match k.to_ascii_lowercase().as_str() {
                    "range" => range = Some(v.trim().to_string()),
                    "authorization" => auth = Some(v.trim().to_string()),
                    _ => {}
                }
            }
        }
        seen.lock().unwrap().push((url.clone(), range.clone()));
        let b = body();
        let head = |status: &str, extra: &str, len: usize| {
            format!("HTTP/1.1 {status}\r\n{extra}content-length: {len}\r\nconnection: close\r\n\r\n")
        };
        if auth.as_deref() != Some("token k:s") {
            let _ = stream.write_all(head("403 Forbidden", "", 0).as_bytes());
            return;
        }
        let start = range
            .as_deref()
            .and_then(|r| r.strip_prefix("bytes=")?.strip_suffix('-')?.parse::<usize>().ok());
        if url == "/flaky" && drops.load(Ordering::SeqCst) > 0 {
            // drop the connection halfway through the body
            drops.fetch_sub(1, Ordering::SeqCst);
            let _ = stream.write_all(head("200 OK", "", b.len()).as_bytes());
            let _ = stream.write_all(&b[..5000]);
            let _ = stream.flush();
            return;
        }
        if let (true, Some(start)) = (url == "/ranged" || url == "/flaky", start) {
            if start >= b.len() {
                let extra = format!("content-range: bytes */{}\r\n", b.len());
                let _ = stream.write_all(head("416 Range Not Satisfiable", &extra, 0).as_bytes());
                return;
            }
            let extra = format!("content-range: bytes {start}-{}/{}\r\n", b.len() - 1, b.len());
            let _ = stream.write_all(head("206 Partial Content", &extra, b.len() - start).as_bytes());
            let _ = stream.write_all(&b[start..]);
            return;
        }
        if url == "/ranged" || url == "/plain" || url == "/flaky" {
            let _ = stream.write_all(head("200 OK", "", b.len()).as_bytes());
            let _ = stream.write_all(&b);
            return;
        }
        let _ = stream.write_all(head("404 Not Found", "", 0).as_bytes());
    }

    fn opts(log: &dyn Fn(&str)) -> DownloadOptions<'_> {
        DownloadOptions {
            headers: vec![("Authorization".into(), "token k:s".into())],
            log,
            attempts: 1,
            backoff_ms: 1,
            progress: false,
        }
    }

    #[test]
    fn resumes_a_partial_from_where_it_stopped() {
        let (srv, dir) = (serve(), tempfile::tempdir().unwrap());
        let dest = dir.path().join("a.gz");
        fs::write(dir.path().join("a.gz.part"), &body()[..4000]).unwrap();
        download_resumable(&format!("{}/ranged", srv.base), &dest, &opts(&|_| {})).unwrap();
        assert_eq!(srv.seen.lock().unwrap()[0].1.as_deref(), Some("bytes=4000-"));
        assert_eq!(fs::read(&dest).unwrap(), body());
        assert!(!dir.path().join("a.gz.part").exists());
    }

    #[test]
    fn whole_partial_416_is_kept_not_redownloaded() {
        let (srv, dir) = (serve(), tempfile::tempdir().unwrap());
        let dest = dir.path().join("whole.gz");
        fs::write(dir.path().join("whole.gz.part"), body()).unwrap();
        download_resumable(&format!("{}/ranged", srv.base), &dest, &opts(&|_| {})).unwrap();
        assert_eq!(srv.seen.lock().unwrap().len(), 1);
        assert_eq!(fs::read(&dest).unwrap(), body());
    }

    #[test]
    fn connection_dropped_mid_body_is_retried_to_a_complete_file() {
        let (srv, dir) = (serve(), tempfile::tempdir().unwrap());
        let dest = dir.path().join("flaky.gz");
        srv.flaky_drops.store(1, Ordering::SeqCst);
        let o = DownloadOptions {
            attempts: 2,
            backoff_ms: 0,
            ..opts(&|_| {})
        };
        download_resumable(&format!("{}/flaky", srv.base), &dest, &o).unwrap();
        assert_eq!(srv.seen.lock().unwrap().len(), 2);
        assert_eq!(fs::read(&dest).unwrap(), body());
        assert!(!dir.path().join("flaky.gz.part").exists());
    }

    #[test]
    fn gives_up_after_attempts_without_leaving_dest() {
        let (srv, dir) = (serve(), tempfile::tempdir().unwrap());
        let dest = dir.path().join("flaky-out.gz");
        srv.flaky_drops.store(2, Ordering::SeqCst);
        let o = DownloadOptions {
            attempts: 2,
            backoff_ms: 0,
            ..opts(&|_| {})
        };
        let e = download_resumable(&format!("{}/flaky", srv.base), &dest, &o).unwrap_err();
        assert!(e.message.contains("download failed"), "{}", e.message);
        assert_eq!(srv.seen.lock().unwrap().len(), 2);
        assert!(!dest.exists());
    }

    #[test]
    fn restarts_from_zero_when_source_ignores_range() {
        let (srv, dir) = (serve(), tempfile::tempdir().unwrap());
        let dest = dir.path().join("b.gz");
        fs::write(dir.path().join("b.gz.part"), "garbage that must not survive").unwrap();
        download_resumable(&format!("{}/plain", srv.base), &dest, &opts(&|_| {})).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), body());
    }

    #[test]
    fn restarts_when_partial_is_past_the_end() {
        let (srv, dir) = (serve(), tempfile::tempdir().unwrap());
        let dest = dir.path().join("c.gz");
        let mut over = body();
        over.extend_from_slice(b"extra");
        fs::write(dir.path().join("c.gz.part"), over).unwrap();
        download_resumable(&format!("{}/ranged", srv.base), &dest, &opts(&|_| {})).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), body());
    }

    #[test]
    fn http_errors_fail_without_dest_and_4xx_is_not_retried() {
        let (srv, dir) = (serve(), tempfile::tempdir().unwrap());
        let dest = dir.path().join("d.gz");
        let o = DownloadOptions {
            attempts: 3,
            ..opts(&|_| {})
        };
        let e = download_resumable(&format!("{}/missing", srv.base), &dest, &o).unwrap_err();
        assert!(e.message.contains("HTTP 404"), "{}", e.message);
        assert_eq!(srv.seen.lock().unwrap().len(), 1);
        assert!(!dest.exists());
        let o = DownloadOptions {
            headers: vec![],
            ..opts(&|_| {})
        };
        let e = download_resumable(&format!("{}/plain", srv.base), &dest, &o).unwrap_err();
        assert!(e.message.contains("HTTP 403"), "{}", e.message);
    }

    #[test]
    fn gzip_intact_accepts_whole_rejects_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&body()).unwrap();
        let gz = enc.finish().unwrap();
        let (good, cut) = (dir.path().join("good.gz"), dir.path().join("cut.gz"));
        fs::write(&good, &gz).unwrap();
        fs::write(&cut, &gz[..gz.len() - 10]).unwrap();
        assert!(gzip_intact(&good));
        assert!(!gzip_intact(&cut));
    }
}
