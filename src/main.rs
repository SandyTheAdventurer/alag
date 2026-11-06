//! The `alag` command: serves the viewer page and the experiment files on localhost.
//!
//! The server stays simple by not parsing metrics. The page asks for
//! `metrics.csv` from a byte offset, so each update sends only the new rows.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use std::{env, process, thread};

const PAGE: &str = include_str!("index.html");
const DEFAULT_PORT: u16 = 6100;
const PORT_ATTEMPTS: u16 = 20;
const MAX_CHUNK: u64 = 16 * 1024 * 1024;

const HELP: &str = "Serve ALAG experiments on localhost.

Usage: alag [LOGDIR] [--port PORT]

  LOGDIR       Folder of logged experiments (default: logs)
  --port PORT  Port to use (default: 6100, or the next free one)
  --version    Print the version
  --help       Print this help";

struct Reply {
    content_type: &'static str,
    body: Vec<u8>,
}

fn main() {
    let (root, port) = parse_args();
    let (listener, port) = bind(port);

    if !root.is_dir() {
        eprintln!("note: {} doesn't exist yet", root.display());
    }
    println!("ALAG is serving {} at http://localhost:{port}", root.display());

    let root = Arc::new(root);
    for stream in listener.incoming().flatten() {
        let root = Arc::clone(&root);
        thread::spawn(move || {
            let _ = handle(stream, &root);
        });
    }
}

fn parse_args() -> (PathBuf, Option<u16>) {
    let mut root = None;
    let mut port = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("alag {}", env!("CARGO_PKG_VERSION"));
                process::exit(0);
            }
            "--port" => match args.next().and_then(|p| p.parse().ok()) {
                Some(p) => port = Some(p),
                None => fail("--port needs a number"),
            },
            flag if flag.starts_with('-') => fail(&format!("unknown option {flag}")),
            path if root.is_none() => root = Some(PathBuf::from(path)),
            _ => fail("only one LOGDIR is allowed"),
        }
    }
    (root.unwrap_or_else(|| PathBuf::from("logs")), port)
}

fn fail(message: &str) -> ! {
    eprintln!("error: {message}\n\n{HELP}");
    process::exit(2);
}

/// Binds to the loopback address only, so experiments aren't exposed to the network.
fn bind(port: Option<u16>) -> (TcpListener, u16) {
    let (first, attempts) = match port {
        Some(p) => (p, 1),
        None => (DEFAULT_PORT, PORT_ATTEMPTS),
    };
    for p in first..first.saturating_add(attempts) {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", p)) {
            return (listener, p);
        }
    }
    eprintln!("error: couldn't find a free port starting at {first}");
    process::exit(1);
}

fn handle(mut stream: TcpStream, root: &Path) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?.take(16 * 1024));
    let mut request = String::new();
    reader.read_line(&mut request)?;
    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 && line.trim() != "" {
        line.clear();
    }

    let mut parts = request.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let result = if method == "GET" {
        route(target, root)
    } else {
        Err(405)
    };

    let (status, reply) = match result {
        Ok(reply) => ("200 OK", reply),
        Err(code) => {
            let status = match code {
                400 => "400 Bad Request",
                405 => "405 Method Not Allowed",
                _ => "404 Not Found",
            };
            let body = status.as_bytes().to_vec();
            (status, Reply { content_type: "text/plain", body })
        }
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        reply.content_type,
        reply.body.len()
    )?;
    stream.write_all(&reply.body)
}

fn route(target: &str, root: &Path) -> Result<Reply, u16> {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let params: Vec<(String, String)> = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (percent_decode(k), percent_decode(v)))
        .collect();
    let param = |key: &str| {
        params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .ok_or(400u16)
    };
    let experiment = || resolve(root, param("g")?, param("e")?);

    match path {
        "/" => Ok(Reply {
            content_type: "text/html; charset=utf-8",
            body: PAGE.as_bytes().to_vec(),
        }),
        "/api/tree" => Ok(json(tree(root))),
        "/api/metrics" => {
            let offset = param("offset")?.parse().map_err(|_| 400u16)?;
            metrics(&experiment()?, offset)
        }
        "/api/config" => {
            let body = fs::read(experiment()?.join("config.json")).map_err(|_| 404u16)?;
            Ok(Reply { content_type: "application/json", body })
        }
        "/api/images" => Ok(json(images(&experiment()?))),
        "/file" => file(root, &experiment()?, param("p")?),
        _ => Err(404),
    }
}

fn json(body: String) -> Reply {
    Reply { content_type: "application/json", body: body.into_bytes() }
}

/// Returns the folder for a group and experiment name, refusing anything that
/// would step outside the log directory.
fn resolve(root: &Path, group: &str, experiment: &str) -> Result<PathBuf, u16> {
    if !is_plain_name(group) || !is_plain_name(experiment) {
        return Err(400);
    }
    Ok(root.join(group).join(experiment))
}

fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\'])
}

fn tree(root: &Path) -> String {
    let groups: Vec<String> = sorted_dirs(root)
        .into_iter()
        .filter_map(|group| {
            let experiments: Vec<String> = sorted_dirs(&group)
                .iter()
                .filter_map(|dir| experiment_json(dir))
                .collect();
            if experiments.is_empty() {
                return None;
            }
            Some(format!(
                r#"{{"name":{},"experiments":[{}]}}"#,
                quote(&file_name(&group)),
                experiments.join(",")
            ))
        })
        .collect();
    format!(
        r#"{{"root":{},"groups":[{}]}}"#,
        quote(&root.display().to_string()),
        groups.join(",")
    )
}

fn experiment_json(dir: &Path) -> Option<String> {
    let meta = fs::metadata(dir.join("metrics.csv")).ok()?;
    let age = meta
        .modified()
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map_or(0, |d| d.as_secs());
    Some(format!(
        r#"{{"name":{},"size":{},"age":{age}}}"#,
        quote(&file_name(dir)),
        meta.len()
    ))
}

fn sorted_dirs(path: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

/// Reads whole lines from `offset`. The logger may be partway through a line,
/// so anything after the last newline waits for the next request.
fn metrics(dir: &Path, offset: u64) -> Result<Reply, u16> {
    let mut file = File::open(dir.join("metrics.csv")).map_err(|_| 404u16)?;
    file.seek(SeekFrom::Start(offset)).map_err(|_| 400u16)?;
    let mut body = Vec::new();
    file.take(MAX_CHUNK).read_to_end(&mut body).map_err(|_| 404u16)?;
    let complete = body.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    body.truncate(complete);
    Ok(Reply { content_type: "text/csv; charset=utf-8", body })
}

fn images(dir: &Path) -> String {
    let base = dir.join("images");
    let mut found = Vec::new();
    collect_images(&base, &base, &mut found);
    found.sort();
    format!("[{}]", found.join(","))
}

fn collect_images(base: &Path, dir: &Path, found: &mut Vec<String>) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_images(base, &path, found);
            continue;
        }
        let Some(step) = path.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u64>().ok())
        else {
            continue;
        };
        if content_type(&path).is_none() {
            continue;
        }
        let name = path.parent().and_then(|p| p.strip_prefix(base).ok());
        let relative = path.strip_prefix(base).ok();
        if let (Some(name), Some(relative)) = (name, relative) {
            found.push(format!(
                r#"{{"name":{},"step":{step},"path":{}}}"#,
                quote(&slashes(name)),
                quote(&format!("images/{}", slashes(relative)))
            ));
        }
    }
}

fn slashes(path: &Path) -> String {
    let parts: Vec<_> = path.iter().map(|p| p.to_string_lossy()).collect();
    parts.join("/")
}

fn file(root: &Path, dir: &Path, relative: &str) -> Result<Reply, u16> {
    if !relative.split('/').all(is_plain_name) {
        return Err(400);
    }
    let path = dir.join(relative);
    let content_type = content_type(&path).ok_or(404u16)?;
    // Canonical paths also catch a symlink that points out of the log directory.
    let real = path.canonicalize().map_err(|_| 404u16)?;
    let real_root = root.canonicalize().map_err(|_| 404u16)?;
    if !real.starts_with(real_root) {
        return Err(404);
    }
    let body = fs::read(real).map_err(|_| 404u16)?;
    Ok(Reply { content_type, body })
}

fn content_type(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        _ => None,
    }
}

fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%')
            .then(|| bytes.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (hex, bytes[i]) {
            (Some(byte), _) => {
                out.push(byte);
                i += 3;
            }
            (None, b'+') => {
                out.push(b' ');
                i += 1;
            }
            (None, byte) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_outside_the_log_directory() {
        let root = Path::new("logs");
        assert!(resolve(root, "..", "x").is_err());
        assert!(resolve(root, "a/b", "x").is_err());
        assert!(resolve(root, "a", "..").is_err());
        assert!(resolve(root, "a", "b").is_ok());
    }

    #[test]
    fn decodes_query_values() {
        assert_eq!(percent_decode("a%2Fb+c"), "a/b c");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn escapes_json() {
        assert_eq!(quote("a\"b\\c\n"), r#""a\"b\\c\u000a""#);
    }
}
