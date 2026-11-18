//! A local update feed for the release check: one folder (a test build's release, in the GitHub API's
//! shape, and its zip) served on 127.0.0.1 as the exe's updater reads a feed. GET and HEAD, one byte range at most (a request for
//! several gets the whole file); one line per request.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

/// A running feed: its address and the thread that accepts.
pub struct Feed {
    pub addr: SocketAddr,
    pub thread: JoinHandle<()>,
}

impl Feed {
    /// The feed's URL, ending in `/` (what `MVS_UPDATE_FEED` takes).
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.addr.port())
    }
}

/// Serves `root` on 127.0.0.1 (`port` 0: any free port) until the process ends; each request on a thread of its own.
pub fn start(root: &Path, port: u16) -> std::io::Result<Feed> {
    let root = root.canonicalize()?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
    let addr = listener.local_addr()?;
    let thread = std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let root = root.clone();
            std::thread::spawn(move || {
                if let Err(e) = answer(&root, stream) {
                    println!("serve: {e}");
                }
            });
        }
    });
    Ok(Feed { addr, thread })
}

/// `%xx` escapes decoded; None when one does not read or the result is not UTF-8.
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The file a request path names under `root`; None for a path that leaves it or names no file.
fn file_of(root: &Path, target: &str) -> Option<PathBuf> {
    let path = target.split(['?', '#']).next()?;
    let path = percent_decode(path)?;
    let mut file = root.to_path_buf();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        if part == "." || part == ".." || part.contains(['\\', ':']) {
            return None;
        }
        file.push(part);
    }
    file.is_file().then_some(file)
}

/// The byte range a `Range` header asks for, of a file of `size` bytes: None for the whole file, Err for one past
/// its end (416).
fn range_of(header: Option<&str>, size: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else { return Ok(None) };
    let Some((a, b)) = spec.split_once('-') else { return Ok(None) };
    if spec.contains(',') || (a.is_empty() && b.is_empty()) {
        return Ok(None);
    }
    let (Ok(a), Ok(b)) = (
        if a.is_empty() { Ok(None) } else { a.parse::<u64>().map(Some) },
        if b.is_empty() { Ok(None) } else { b.parse::<u64>().map(Some) },
    ) else {
        return Ok(None);
    };
    let last = size.checked_sub(1);
    let (start, end) = match (a, b) {
        (Some(a), b) => (a, b.map_or(last, |b| last.map(|l| l.min(b)))),
        (None, Some(n)) => (size.saturating_sub(n), last),
        (None, None) => return Ok(None),
    };
    match end {
        Some(end) if start <= end => Ok(Some((start, end))),
        _ => Err(()),
    }
}

fn answer(root: &Path, mut stream: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or("").to_owned(), parts.next().unwrap_or("/").to_owned());
    let mut range = None;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':')
            && k.trim().eq_ignore_ascii_case("range")
        {
            range = Some(v.trim().to_owned());
        }
    }
    let mut status = |code: u16, text: &str, extra: &str| -> std::io::Result<()> {
        println!("serve: {method} {target} {code}");
        write!(stream, "HTTP/1.1 {code} {text}\r\ncontent-length: 0\r\nconnection: close\r\n{extra}\r\n")
    };
    if method != "GET" && method != "HEAD" {
        return status(405, "Method Not Allowed", "");
    }
    let Some(file) = file_of(root, &target) else { return status(404, "Not Found", "") };
    let mut f = File::open(&file)?;
    let size = f.metadata()?.len();
    let (code, text, start, end) = match range_of(range.as_deref(), size) {
        Ok(None) => (200, "OK", 0, size),
        Ok(Some((a, b))) => (206, "Partial Content", a, b + 1),
        Err(()) => return status(416, "Range Not Satisfiable", &format!("content-range: bytes */{size}\r\n")),
    };
    let kind = match file.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("json") => "application/json",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    };
    let mut head = format!(
        "HTTP/1.1 {code} {text}\r\ncontent-type: {kind}\r\naccept-ranges: bytes\r\ncontent-length: {}\r\nconnection: close\r\n",
        end - start
    );
    if code == 206 {
        head += &format!("content-range: bytes {start}-{}/{size}\r\n", end - 1);
        println!("serve: {method} {target} 206 {start}-{}", end - 1);
    } else {
        println!("serve: {method} {target} 200");
    }
    head += "\r\n";
    stream.write_all(head.as_bytes())?;
    if method == "GET" {
        f.seek(SeekFrom::Start(start))?;
        std::io::copy(&mut f.take(end - start), &mut stream)?;
    }
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(feed: &Feed, path: &str, extra: &str) -> (String, Vec<u8>) {
        let mut s = TcpStream::connect(feed.addr).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nhost: x\r\n{extra}\r\n").unwrap();
        let mut all = Vec::new();
        s.read_to_end(&mut all).unwrap();
        let at = all.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        (String::from_utf8_lossy(&all[..at]).into_owned(), all[at + 4..].to_vec())
    }

    #[test]
    fn a_folder_is_served() {
        let dir = std::env::temp_dir().join(format!("mvs-dist-serve-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("latest.json"), b"{\"v\":1}").unwrap();
        std::fs::write(dir.join("a b.zip"), b"0123456789").unwrap();
        let feed = start(&dir, 0).unwrap();
        let (head, body) = get(&feed, "/latest.json", "");
        assert!(head.starts_with("HTTP/1.1 200") && head.contains("application/json"));
        assert_eq!(body, b"{\"v\":1}");
        let (head, body) = get(&feed, "/a%20b.zip", "range: bytes=2-4\r\n");
        assert!(head.starts_with("HTTP/1.1 206") && head.contains("content-range: bytes 2-4/10"));
        assert_eq!(body, b"234");
        assert_eq!(get(&feed, "/a%20b.zip", "range: bytes=-3\r\n").1, b"789");
        assert!(get(&feed, "/a%20b.zip", "range: bytes=10-\r\n").0.starts_with("HTTP/1.1 416"));
        assert!(get(&feed, "/../Cargo.toml", "").0.starts_with("HTTP/1.1 404"));
        assert!(get(&feed, "/%2e%2e/x", "").0.starts_with("HTTP/1.1 404"));
        assert!(get(&feed, "/none.zip", "").0.starts_with("HTTP/1.1 404"));
        std::fs::remove_file(dir.join("latest.json")).unwrap();
        std::fs::remove_file(dir.join("a b.zip")).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
}
