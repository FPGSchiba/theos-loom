//! A fake Claude Code CLI for `loom-run`'s tests (spec R-m, R-p). Test tooling, not shipped code.
//!
//! - `update`: exits 0, or 1 when `FAKE_UPDATE_FAILS=1`.
//! - `--version`: prints `9.9.9 (Fake)`.
//! - `-p …`: reads stdin, then acts by `FAKE_MODE`:
//!   - `replay`: writes `FAKE_STDERR` to stderr, then prints the file `FAKE_STREAM` line by line,
//!     `FAKE_DELAY_MS` apart; then sleeps an hour when `FAKE_THEN=sleep`, else exits with
//!     `FAKE_EXIT` (default 0).
//!   - `sleep`: prints one `system`/`init` line, then sleeps an hour.
//!   - `spawn`: starts two children of itself in `sleep` mode, prints their pids, then sleeps.
//!   - `report`: prints one `fake_report` line: uid, cwd, branch, kit version, args, stdin and the
//!     `LOOM_TEST_*` variables.
//!   - `request`: an HTTP GET of `FAKE_URL` through `HTTP_PROXY`; prints its status or error.
//!   - `connect`: a direct TCP connection to `FAKE_ADDR`; prints whether it worked.
//!   - `serve`: answers every HTTP request on `0.0.0.0:FAKE_PORT` with 200 `ok`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

const HOUR: Duration = Duration::from_secs(3600);
const NET_TIMEOUT: Duration = Duration::from_secs(10);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("update") => update(),
        Some("--version") => out("9.9.9 (Fake)\n").map(|()| ExitCode::SUCCESS),
        Some("-p") => prompt(&args),
        _ => Err(format!("fake_claude: unknown arguments {args:?}")),
    };
    match result {
        Ok(code) => code,
        Err(message) => {
            // Nothing more can be done when stderr is gone.
            let _ = writeln!(std::io::stderr().lock(), "{message}");
            ExitCode::from(2)
        }
    }
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn out(text: &str) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(|e| format!("stdout: {e}"))
}

fn line(value: &Value) -> Result<(), String> {
    out(&format!("{value}\n"))
}

fn update() -> Result<ExitCode, String> {
    if var("FAKE_UPDATE_FAILS").as_deref() == Some("1") {
        let _ = writeln!(std::io::stderr().lock(), "update failed (fake)");
        return Ok(ExitCode::FAILURE);
    }
    out("Up to date (fake)\n")?;
    Ok(ExitCode::SUCCESS)
}

fn prompt(args: &[String]) -> Result<ExitCode, String> {
    let mut stdin = String::new();
    std::io::stdin()
        .read_to_string(&mut stdin)
        .map_err(|e| format!("stdin: {e}"))?;
    match var("FAKE_MODE").as_deref().unwrap_or("replay") {
        "replay" => replay(),
        "sleep" => sleep(),
        "spawn" => spawn(),
        "report" => report(args, &stdin),
        "request" => request(),
        "connect" => connect(),
        "serve" => serve(),
        other => Err(format!("fake_claude: unknown FAKE_MODE {other:?}")),
    }
}

fn replay() -> Result<ExitCode, String> {
    if let Some(text) = var("FAKE_STDERR") {
        let mut stderr = std::io::stderr().lock();
        stderr
            .write_all(text.as_bytes())
            .and_then(|()| stderr.flush())
            .map_err(|e| format!("stderr: {e}"))?;
    }
    let path = var("FAKE_STREAM").ok_or("FAKE_STREAM is not set")?;
    let body = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    let delay = var("FAKE_DELAY_MS")
        .and_then(|ms| ms.parse().ok())
        .map(Duration::from_millis);
    let mut stdout = std::io::stdout().lock();
    for chunk in body.split_inclusive(|b| *b == b'\n') {
        stdout
            .write_all(chunk)
            .and_then(|()| stdout.flush())
            .map_err(|e| format!("stdout: {e}"))?;
        if let Some(delay) = delay {
            std::thread::sleep(delay);
        }
    }
    drop(stdout);
    if var("FAKE_THEN").as_deref() == Some("sleep") {
        std::thread::sleep(HOUR);
    }
    let code: u8 = var("FAKE_EXIT").and_then(|c| c.parse().ok()).unwrap_or(0);
    Ok(ExitCode::from(code))
}

fn sleep() -> Result<ExitCode, String> {
    line(&json!({"type": "system", "subtype": "init", "session_id": "fake", "model": "fake"}))?;
    std::thread::sleep(HOUR);
    Ok(ExitCode::SUCCESS)
}

fn spawn() -> Result<ExitCode, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let mut pids = Vec::new();
    for _ in 0..2 {
        let child = Command::new(&exe)
            .arg("-p")
            .env("FAKE_MODE", "sleep")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn: {e}"))?;
        pids.push(child.id());
    }
    line(&json!({"type": "fake_spawn", "pids": pids}))?;
    std::thread::sleep(HOUR);
    Ok(ExitCode::SUCCESS)
}

fn report(args: &[String], stdin: &str) -> Result<ExitCode, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cwd: {e}"))?;
    let branch = std::fs::read_to_string(cwd.join(".git").join("HEAD"))
        .ok()
        .map(|head| {
            let head = head.trim();
            head.strip_prefix("ref: refs/heads/")
                .unwrap_or(head)
                .to_owned()
        });
    let kit_version = var("KIT_DIR")
        .and_then(|dir| std::fs::read_to_string(Path::new(&dir).join("VERSION")).ok())
        .map(|v| v.trim().to_owned());
    let env: serde_json::Map<String, Value> = std::env::vars()
        .filter(|(name, _)| name.starts_with("LOOM_TEST_"))
        .map(|(name, value)| (name, Value::String(value)))
        .collect();
    line(&json!({
        "type": "fake_report",
        "uid": uid(),
        "cwd": cwd.display().to_string(),
        "branch": branch,
        "kit_version": kit_version,
        "args": args,
        "stdin": stdin,
        "env": env,
    }))?;
    Ok(ExitCode::SUCCESS)
}

/// The real uid of this process, from `/proc/self/status`.
#[cfg(target_os = "linux")]
fn uid() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let uids = status.lines().find_map(|l| l.strip_prefix("Uid:"))?;
    uids.split_whitespace().next()?.parse().ok()
}

#[cfg(not(target_os = "linux"))]
fn uid() -> Option<u64> {
    None
}

fn request() -> Result<ExitCode, String> {
    let result = http_get_via_proxy();
    let value = match result {
        Ok(status) => json!({"type": "fake_request", "status": status}),
        Err(error) => json!({"type": "fake_request", "error": error}),
    };
    line(&value)?;
    Ok(ExitCode::SUCCESS)
}

fn http_get_via_proxy() -> Result<u16, String> {
    let url = var("FAKE_URL").ok_or("FAKE_URL is not set")?;
    let proxy = var("HTTP_PROXY")
        .or_else(|| var("http_proxy"))
        .ok_or("HTTP_PROXY is not set")?;
    let proxy_addr = proxy
        .strip_prefix("http://")
        .unwrap_or(&proxy)
        .trim_end_matches('/');
    let host = url
        .strip_prefix("http://")
        .and_then(|rest| rest.split('/').next())
        .ok_or("FAKE_URL must start with http://")?;
    let mut stream = open(proxy_addr)?;
    let request = format!("GET {url} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let mut status_line = String::new();
    BufReader::new(stream)
        .read_line(&mut status_line)
        .map_err(|e| format!("read: {e}"))?;
    status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| format!("no HTTP status in {status_line:?}"))
}

fn open(addr: &str) -> Result<TcpStream, String> {
    let socket = addr
        .to_socket_addrs()
        .map_err(|e| format!("resolve {addr}: {e}"))?
        .next()
        .ok_or_else(|| format!("{addr} resolves to nothing"))?;
    let stream =
        TcpStream::connect_timeout(&socket, NET_TIMEOUT).map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(NET_TIMEOUT))
        .map_err(|e| format!("timeout: {e}"))?;
    Ok(stream)
}

fn connect() -> Result<ExitCode, String> {
    let addr = var("FAKE_ADDR").ok_or("FAKE_ADDR is not set")?;
    let ok = open(&addr).is_ok();
    line(&json!({"type": "fake_connect", "ok": ok}))?;
    Ok(ExitCode::SUCCESS)
}

fn serve() -> Result<ExitCode, String> {
    let port = var("FAKE_PORT").ok_or("FAKE_PORT is not set")?;
    let listener =
        TcpListener::bind(format!("0.0.0.0:{port}")).map_err(|e| format!("bind: {e}"))?;
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let _ = stream.set_read_timeout(Some(NET_TIMEOUT));
        let mut head = BufReader::new(&stream);
        let mut request_line = String::new();
        // Read the request head up to its empty line; a broken client is simply dropped.
        while head.read_line(&mut request_line).is_ok_and(|n| n > 2) {
            request_line.clear();
        }
        let _ = stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
    }
    Ok(ExitCode::SUCCESS)
}
