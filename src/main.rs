//! The `alag` command: serves the viewer page and the experiment files on localhost.
//!
//! The server stays simple by not parsing metrics. The page asks for
//! `metrics.csv` from a byte offset, so each update sends only the new rows.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, process, thread};

const PAGE: &str = include_str!("index.html");
const DEFAULT_PORT: u16 = 6100;
const PORT_ATTEMPTS: u16 = 20;
const MAX_CHUNK: u64 = 16 * 1024 * 1024;
const MAX_BODY: usize = 1024 * 1024;
const MAX_HEADERS: u64 = 16 * 1024;

const HELP: &str = "Serve ALAG experiments on localhost.

Usage: alag [LOGDIR] [--port PORT]
       alag [LOGDIR] --export FILE

  LOGDIR         Folder of logged experiments (default: logs)
  --port PORT    Port to use (default: 6100, or the next free one)
  --export FILE  Write everything in LOGDIR to one read-only HTML file
  --version      Print the version
  --help         Print this help";

struct Reply {
    content_type: &'static str,
    body: Vec<u8>,
}

fn main() {
    let (root, port, output) = parse_args();
    if let Some(output) = output {
        export(&root, &output);
        return;
    }
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

fn parse_args() -> (PathBuf, Option<u16>, Option<PathBuf>) {
    let mut root = None;
    let mut port = None;
    let mut output = None;
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
            "--export" => match args.next() {
                Some(path) => output = Some(PathBuf::from(path)),
                None => fail("--export needs a file name"),
            },
            flag if flag.starts_with('-') => fail(&format!("unknown option {flag}")),
            path if root.is_none() => root = Some(PathBuf::from(path)),
            _ => fail("only one LOGDIR is allowed"),
        }
    }
    if port.is_some() && output.is_some() {
        fail("--port and --export can't be used together");
    }
    (root.unwrap_or_else(|| PathBuf::from("logs")), port, output)
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
    let mut reader = BufReader::new(stream.try_clone()?.take(MAX_HEADERS + MAX_BODY as u64));
    let mut request = String::new();
    reader.read_line(&mut request)?;
    let (mut length, mut host, mut marked) = (0, String::new(), false);
    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 && line.trim() != "" {
        if let Some((name, value)) = line.split_once(':') {
            match name.to_ascii_lowercase().as_str() {
                "content-length" => length = value.trim().parse().unwrap_or(usize::MAX),
                "host" => host = value.trim().to_string(),
                "x-alag-write" => marked = true,
                _ => {}
            }
        }
        line.clear();
    }

    let mut parts = request.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let result = match method {
        "GET" => route(target, root),
        // Any website can send requests to a localhost port. Browsers won't send
        // this header cross-site without a preflight, which this server refuses,
        // and the Host check stops a site that rebinds its own name to 127.0.0.1.
        "POST" if !marked || !is_local_host(&host) => Err(403),
        "POST" if length > MAX_BODY => Err(400),
        "POST" => {
            let mut body = vec![0; length];
            reader.read_exact(&mut body)?;
            save(target, root, &body)
        }
        _ => Err(405),
    };

    let (status, reply) = match result {
        Ok(reply) => ("200 OK", reply),
        Err(code) => {
            let status = match code {
                400 => "400 Bad Request",
                403 => "403 Forbidden",
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

fn is_local_host(host: &str) -> bool {
    let name = match host.rsplit_once(':') {
        Some((name, port)) if port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    };
    matches!(name, "localhost" | "127.0.0.1" | "[::1]")
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (percent_decode(k), percent_decode(v)))
        .collect()
}

fn find<'a>(params: &'a [(String, String)], key: &str) -> Result<&'a str, u16> {
    params
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .ok_or(400)
}

/// Stores notes, tags, flags and chart layout. These files are the only ones the
/// page can change, and only in folders that already exist.
fn save(target: &str, root: &Path, body: &[u8]) -> Result<Reply, u16> {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let params = parse_query(query);
    let group = find(&params, "g")?;
    let file = match path {
        "/api/meta" => resolve(root, group, find(&params, "e")?)?.join("meta.json"),
        "/api/flags" if is_plain_name(group) => root.join(group).join("flags.json"),
        "/api/layout" if is_plain_name(group) => root.join(group).join("layout.json"),
        _ => return Err(404),
    };
    if !file.parent().is_some_and(Path::is_dir) {
        return Err(404);
    }
    write_atomic(&file, body).map_err(|_| 404u16)?;
    Ok(json("{}".to_string()))
}

fn write_atomic(path: &Path, body: &[u8]) -> std::io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let tmp = path.with_extension(format!("{}.tmp", NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&tmp, body)?;
    fs::rename(&tmp, path)
}

fn route(target: &str, root: &Path) -> Result<Reply, u16> {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let params = parse_query(query);
    let param = |key: &str| find(&params, key);
    let experiment = || resolve(root, param("g")?, param("e")?);

    match path {
        "/" => Ok(Reply {
            content_type: "text/html; charset=utf-8",
            body: PAGE.as_bytes().to_vec(),
        }),
        "/api/tree" => Ok(json(tree(root, &root.display().to_string()))),
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

fn tree(root: &Path, label: &str) -> String {
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
                r#"{{"name":{},"flags":{},"layout":{},"experiments":[{}]}}"#,
                quote(&file_name(&group)),
                quote(&read_text(&group.join("flags.json"))),
                quote(&read_text(&group.join("layout.json"))),
                experiments.join(",")
            ))
        })
        .collect();
    format!(
        r#"{{"root":{},"groups":[{}]}}"#,
        quote(label),
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
        r#"{{"name":{},"size":{},"age":{age},"meta":{}}}"#,
        quote(&file_name(dir)),
        meta.len(),
        quote(&read_text(&dir.join("meta.json")))
    ))
}

/// Notes, flags and layout go to the page as strings for it to parse, so a hand-edited
/// file with a typo can't break the whole experiment list.
fn read_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
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

struct Image {
    name: String,
    step: u64,
    path: String,
}

fn image_list(dir: &Path) -> Vec<Image> {
    let base = dir.join("images");
    let mut found = Vec::new();
    collect_images(&base, &base, &mut found);
    found.sort_by(|a, b| (&a.name, a.step).cmp(&(&b.name, b.step)));
    found
}

fn images(dir: &Path) -> String {
    let items: Vec<String> = image_list(dir)
        .iter()
        .map(|i| {
            format!(r#"{{"name":{},"step":{},"path":{}}}"#, quote(&i.name), i.step, quote(&i.path))
        })
        .collect();
    format!("[{}]", items.join(","))
}

fn collect_images(base: &Path, dir: &Path, found: &mut Vec<Image>) {
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
            found.push(Image {
                name: slashes(name),
                step,
                path: format!("images/{}", slashes(relative)),
            });
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

/// Writes the page with every experiment inside it, so the file needs no server.
fn export(root: &Path, output: &Path) {
    let Some(html) = export_html(root) else {
        eprintln!("error: no experiments in {}", root.display());
        process::exit(1);
    };
    if let Err(error) = write_atomic(output, html.as_bytes()) {
        eprintln!("error: couldn't write {}: {error}", output.display());
        process::exit(1);
    }
    let size = html.len() as f64 / 1e6;
    println!("Exported {} to {} ({size:.1} MB)", root.display(), output.display());
}

fn export_html(root: &Path) -> Option<String> {
    let mut experiments = Vec::new();
    for group in sorted_dirs(root) {
        for dir in sorted_dirs(&group) {
            if let Some(entry) = exported_experiment(root, &group, &dir) {
                experiments.push(entry);
            }
        }
    }
    if experiments.is_empty() {
        return None;
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    // The name only, so a shared file doesn't show where it came from on this machine.
    let label = root.canonicalize().map_or_else(|_| "logs".to_string(), |p| file_name(&p));
    let data = format!(
        r#"{{"exported":{now},"tree":{},"experiments":{{{}}}}}"#,
        tree(root, &label),
        experiments.join(",")
    );
    // Every < is inside a string here. Escaping them stops a metric or note
    // containing </script> from ending the script early.
    let data = data.replace('<', "\\u003c");
    let script = format!("<script>window.ALAG_EXPORT = {data};</script>\n<script>");
    Some(PAGE.replacen("<script>", &script, 1))
}

fn exported_experiment(root: &Path, group: &Path, dir: &Path) -> Option<String> {
    let mut rows = fs::read(dir.join("metrics.csv")).ok()?;
    let complete = rows.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    rows.truncate(complete);

    let images: Vec<String> = image_list(dir)
        .iter()
        .filter_map(|i| {
            let image = file(root, dir, &i.path).ok()?;
            Some(format!(
                r#"{{"name":{},"step":{},"path":{},"data":"data:{};base64,{}"}}"#,
                quote(&i.name),
                i.step,
                quote(&i.path),
                image.content_type,
                base64(&image.body)
            ))
        })
        .collect();
    Some(format!(
        r#"{}:{{"metrics":{},"config":{},"images":[{}]}}"#,
        quote(&format!("{}/{}", file_name(group), file_name(dir))),
        quote(&String::from_utf8_lossy(&rows)),
        quote(&read_text(&dir.join("config.json"))),
        images.join(",")
    ))
}

fn base64(bytes: &[u8]) -> String {
    const LETTERS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let byte = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
        let bits = byte(0) << 16 | byte(1) << 8 | byte(2);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(LETTERS[(bits >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
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
    fn only_local_hosts_can_write() {
        assert!(is_local_host("localhost:6100"));
        assert!(is_local_host("127.0.0.1:6100"));
        assert!(is_local_host("[::1]:6100"));
        assert!(!is_local_host("evil.example:6100"));
        assert!(!is_local_host("localhost.evil.example"));
        assert!(!is_local_host(""));
    }

    #[test]
    fn decodes_query_values() {
        assert_eq!(percent_decode("a%2Fb+c"), "a/b c");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn escapes_json() {
        assert_eq!(quote("a\"b\\c\n\t"), r#""a\"b\\c\n\u0009""#);
    }

    #[test]
    fn encodes_base64() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
    }

    #[test]
    fn export_holds_everything_and_cannot_end_its_script_early() {
        let root = env::temp_dir().join(format!("alag-export-{}", process::id()));
        let dir = root.join("g").join("e");
        fs::create_dir_all(dir.join("images/plot")).unwrap();
        fs::write(
            dir.join("metrics.csv"),
            "step,time,metric,value\n1,2,</script>,3\n4,5,half-writ",
        )
        .unwrap();
        fs::write(dir.join("config.json"), r#"{"lr":0.1}"#).unwrap();
        fs::write(dir.join("images/plot/7.png"), b"foo").unwrap();

        let html = export_html(&root).unwrap();
        let data_end = html.find("</script>").unwrap();
        assert!(html[..data_end].contains("window.ALAG_EXPORT"));
        assert!(html[..data_end].contains(r"1,2,\u003c/script>,3\n"));
        assert!(html[..data_end].contains("data:image/png;base64,Zm9v"));
        assert!(!html[..data_end].contains("half-writ"));
        assert!(html[data_end..].contains("use strict"));

        fs::remove_dir_all(&root).unwrap();
        assert!(export_html(&root).is_none());
    }

    #[test]
    fn page_can_save_layout_but_not_other_files() {
        let root = env::temp_dir().join(format!("alag-save-{}", process::id()));
        fs::create_dir_all(root.join("g")).unwrap();
        assert!(save("/api/layout?g=g", &root, b"{}").is_ok());
        assert!(root.join("g/layout.json").is_file());
        assert!(save("/api/layout?g=..", &root, b"{}").is_err());
        assert!(save("/api/layout?g=missing", &root, b"{}").is_err());
        assert!(save("/api/other?g=g", &root, b"{}").is_err());
        fs::remove_dir_all(&root).unwrap();
    }
}
