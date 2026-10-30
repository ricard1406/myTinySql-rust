//! The small HTTP server behind `--http`: the page's files, a small JSON API, and one thread per connection.
//!
//! The C++ version calls the POSIX socket functions (`socket`, `bind`, `accept`, `recv`...) and wraps
//! the descriptor in its own RAII class. Rust's standard library has `TcpListener` and `TcpStream`,
//! which close themselves when they go out of scope.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use crate::catalog::Catalog;
use crate::output::{render_json, render_tables_json, render_text};
use crate::session::Session;
use crate::util::{parse_integer, trim};

/// Settings of the browser server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// TCP port to listen on.
    pub port: u16,
    /// The directory holding the tables.
    pub data_dir: PathBuf,
    /// The directory holding the page: index.html, style.css, app.js.
    pub web_dir: PathBuf,
    /// IP prefixes such as `192.168.`; empty means everyone.
    pub allowed_clients: Vec<String>,
}

/// The largest request read (headers and body), in bytes; the rest is ignored.
const MAX_REQUEST_SIZE: usize = 64 * 1024;

/// How long a connection may stay silent before it is closed.
///
/// Browsers open connections early ("preconnect") and may send nothing on them for several seconds.
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Lets only one request at a time run SQL and write to the console.
///
/// Each connection is read in its own thread; this lock keeps the table files and the log from being mixed up.
/// It guards no data (`()`), only a stretch of code, like the C++ `std::mutex`.
static WORK: Mutex<()> = Mutex::new(());

/// Takes the [`WORK`] lock; it is released when the returned guard goes out of scope.
///
/// If a thread panicked while holding it, Rust marks the mutex "poisoned" and `lock()` returns an
/// error. There is no data that could be half-changed, so the lock is simply taken anyway.
fn lock_work() -> MutexGuard<'static, ()> {
    WORK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One HTTP request, split into the parts the server uses.
#[derive(Debug, Default)]
struct Request {
    /// E.g. `GET` or `POST`; empty if the request could not be read.
    method: String,
    /// E.g. `/api/sql`; any `?query` part is removed.
    path: String,
    /// The body of a POST (the SQL for `/api/sql`); empty for a GET.
    body: String,
}

/// Finds a header's value in the request line and headers, ignoring the case of its name.
///
/// Returns the value without surrounding spaces, or `""` if the header is missing.
fn header_value<'h>(head: &'h str, name: &str) -> &'h str {
    head.split("\r\n")
        .skip(1) // the request line
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| trim(key).eq_ignore_ascii_case(name))
        .map_or("", |(_, value)| trim(value))
}

/// Finds the first position of `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// Reads one HTTP request: the request line, the headers and, if there is one, the body.
///
/// The method is empty if the client sent nothing before the idle timeout,
/// and the body is cut at [`MAX_REQUEST_SIZE`] bytes.
fn read_request(stream: &mut TcpStream) -> Request {
    let mut raw = Vec::new();
    let mut buffer = [0u8; 4096];
    let mut head_end: Option<usize> = None;
    let mut body_length = 0;
    while raw.len() < MAX_REQUEST_SIZE {
        if head_end.is_none() {
            head_end = find(&raw, b"\r\n\r\n");
            if let Some(end) = head_end {
                // The headers are complete: from now on, read until the body has Content-Length bytes.
                let head = String::from_utf8_lossy(&raw[..end]);
                body_length = parse_integer(header_value(&head, "content-length"))
                    .and_then(|n| usize::try_from(n).ok())
                    .unwrap_or(0);
            }
        }
        if head_end.is_some_and(|end| raw.len() >= end + 4 + body_length) {
            break;
        }
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break, // closed, or silent for longer than the timeout
            Ok(n) => raw.extend_from_slice(&buffer[..n]),
        }
    }

    let Some(head_end) = head_end else {
        return Request::default(); // nothing, or not a complete request
    };
    // Request line: "POST /api/sql HTTP/1.1"
    let head = String::from_utf8_lossy(&raw[..head_end]);
    let line = head.split("\r\n").next().unwrap_or_default();
    let mut parts = line.splitn(3, ' ');
    let (Some(method), Some(target), Some(_version)) = (parts.next(), parts.next(), parts.next()) else {
        return Request::default();
    };
    let body_start = head_end + 4;
    let body_end = (body_start + body_length).min(raw.len());
    Request {
        method: method.to_string(),
        path: target.split('?').next().unwrap_or_default().to_string(),
        body: String::from_utf8_lossy(&raw[body_start..body_end]).into_owned(),
    }
}

/// Builds a complete HTTP response.
///
/// `status` is the status line's code and text, e.g. "200 OK". `no-cache` makes the browser ask again
/// each time, so a change to a file in `web/` shows up on the next reload. The body is bytes, not text,
/// because a file of the page may be an image.
fn http_response(status: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-cache\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

/// Builds a JSON error answer, e.g. `{"error":"Not found"}`.
fn error_response(status: &str, message: &str) -> Vec<u8> {
    http_response(status, "application/json", format!("{{\"error\":\"{message}\"}}").as_bytes())
}

/// The content type of a file of the page, from its extension.
fn content_type_for(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, extension)| extension) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

/// Tells whether a requested file name (without the leading `/`) is safe to serve from the `web/` directory.
///
/// True for plain names such as `app.js`: letters, digits, `.`, `-` and `_`, not starting with `.`.
/// This rules out `..` and `/`, so no request can reach a file outside `web/`.
fn is_safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-' || c == b'_')
}

/// Serves one file of the page from the `web/` directory: the file, or 404 if the name is not safe
/// or the file does not exist.
fn serve_file(config: &ServerConfig, name: &str) -> Vec<u8> {
    if !is_safe_file_name(name) {
        return error_response("404 Not Found", "Not found");
    }
    match fs::read(config.web_dir.join(name)) {
        Ok(content) => http_response("200 OK", content_type_for(name), &content),
        Err(_) => error_response("404 Not Found", "Not found"),
    }
}

/// Runs the SQL of a `POST /api/sql` request, logs it, and builds the JSON answer.
///
/// The `_lock` parameter is never read: asking for a guard makes sure the caller holds [`WORK`].
/// (In C++ that rule is only a comment: "Call it only while holding workMutex".)
fn run_sql(config: &ServerConfig, sql: &str, _lock: &MutexGuard<'_, ()>) -> Vec<u8> {
    let mut session = Session::new(&config.data_dir);
    let results = session.execute_script(sql);
    let mut out = io::stdout().lock();
    for result in &results {
        let _ = out.write_all(render_text(result).as_bytes());
    }
    let _ = out.flush();
    http_response("200 OK", "application/json", render_json(&results).as_bytes())
}

/// Builds the JSON answer of `GET /api/tables`, or a 500 error if SYSTEM_TABLE cannot be read.
/// The caller must hold [`WORK`], as for [`run_sql`].
fn list_tables(config: &ServerConfig, _lock: &MutexGuard<'_, ()>) -> Vec<u8> {
    match Catalog::load(&config.data_dir) {
        Ok(catalog) => http_response("200 OK", "application/json", render_tables_json(catalog.tables()).as_bytes()),
        Err(_) => error_response("500 Internal Server Error", "cannot read the table catalogue"),
    }
}

/// Serves one connection: reads the request, answers it, and closes.
///
/// The routes:
/// - `GET /` and `GET /<file>`: the page, from the `web/` directory;
/// - `GET /api/tables`: the tables and their columns, as JSON;
/// - `POST /api/sql`: runs the SQL in the body, answers with the results as JSON.
///
/// A connection that sends nothing is closed without an answer; a POST elsewhere gets 404, other methods 405.
/// The stream is taken by value: when this function returns, it is dropped, and the connection closed.
fn handle_client(config: &ServerConfig, mut stream: TcpStream, client: &str) {
    let request = read_request(&mut stream);
    if request.method.is_empty() {
        // An idle (preconnected) connection that never sent a request: close it without
        // answering. Had we answered, the browser could later read that answer as the reply
        // to the request it sends on this connection.
        return;
    }

    let response = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/api/tables") => list_tables(config, &lock_work()),
        ("POST", "/api/sql") => {
            let lock = lock_work(); // one request at a time runs SQL
            println!("[{client}] POST /api/sql");
            run_sql(config, &request.body, &lock)
        }
        ("GET", "/") => serve_file(config, "index.html"),
        // `get(1..)` instead of `[1..]`: it gives `None` instead of stopping the program when the
        // path is empty or starts with a character of more than one byte.
        ("GET", path) => serve_file(config, path.get(1..).unwrap_or_default()),
        ("POST", _) => error_response("404 Not Found", "Not found"),
        _ => error_response("405 Method Not Allowed", "Only GET and POST are supported"),
    };
    // The client may have gone away: then there is nobody to tell, so the error is ignored.
    // (Rust ignores the SIGPIPE signal, so this is an error result, not the end of the program;
    // C++ passes MSG_NOSIGNAL to `send` for that.)
    let _ = stream.write_all(&response);
}

/// Serves connections until the process is stopped.
///
/// It serves the page's files from [`ServerConfig::web_dir`] and a small JSON API: `GET /api/tables`
/// lists the tables, `POST /api/sql` runs the SQL in the request body. Each connection is served
/// in its own thread; the SQL itself runs one request at a time.
///
/// Fails if the port cannot be opened (for example, it is already in use).
pub fn run_server(config: ServerConfig) -> io::Result<()> {
    // `TcpListener::bind` also sets SO_REUSEADDR on Unix, as the C++ code does by hand.
    let listener = TcpListener::bind(("0.0.0.0", config.port))
        .map_err(|e| io::Error::new(e.kind(), format!("bind to port {}: {e}", config.port)))?;
    println!("HTTP server listening on port {}", config.port);

    // Every thread needs the settings. `Arc` (atomic reference count) shares one copy between them,
    // and frees it when the last thread is done; C++ copies the whole struct into each thread.
    let config = Arc::new(config);
    loop {
        let (stream, address) = match listener.accept() {
            Ok(connection) => connection,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(io::Error::new(e.kind(), format!("accept: {e}"))),
        };
        let _ = stream.set_read_timeout(Some(IDLE_TIMEOUT));

        let ip = address.ip().to_string();
        let allowed =
            config.allowed_clients.is_empty() || config.allowed_clients.iter().any(|prefix| ip.starts_with(prefix));
        if !allowed {
            let _lock = lock_work();
            println!("[{ip}] rejected: not in the allowed clients");
            continue; // `stream` is dropped here, which closes the connection
        }

        // Serve the connection in its own thread, so a slow or silent client never makes the
        // others wait. `move` hands the stream, the address and a reference to the settings to the
        // thread; the compiler checks that nothing else uses them afterwards. Not keeping the
        // thread's handle is the same as `detach()` in C++.
        let config = Arc::clone(&config);
        thread::spawn(move || handle_client(&config, stream, &ip));
    }
}
