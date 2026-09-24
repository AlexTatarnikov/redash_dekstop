//! In-process fake Redash server, used by tests and by `cargo run -- --mock`.
//!
//! Behaviour (keep AGENTS.md in sync when changing it):
//! - any request without `Authorization: Key test-key` → 403 "Invalid API key"
//! - `GET /api/data_sources` → "Analytics DB" (id 1) and "Events" (id 2)
//! - `POST /api/query_results` → a pending job; SQL containing `fail` makes the job fail
//! - `GET /api/jobs/{id}` → finished job pointing at result 9 (or the failure)
//! - `GET /api/query_results/9` → 40 rows of `id, email, plan, signed_up, mrr`

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{Value, json};

pub const MOCK_API_KEY: &str = "test-key";

pub struct MockRedash {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl MockRedash {
    /// Starts the server on a free localhost port. It runs until the process exits.
    pub fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let url = format!("http://{}", listener.local_addr()?);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&requests);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                // A malformed request only affects that connection.
                let _ = handle(stream, &log);
            }
        });
        Ok(Self { url, requests })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Requests received so far, as `"METHOD /path"`.
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().map(|r| r.clone()).unwrap_or_default()
    }
}

fn handle(stream: TcpStream, log: &Mutex<Vec<String>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();

    let mut content_length = 0;
    let mut auth = String::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header == "\r\n" {
            break;
        }
        let (name, value) = header.split_once(':').unwrap_or((&header, ""));
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => content_length = value.trim().parse().unwrap_or(0),
            "authorization" => auth = value.trim().to_string(),
            _ => {}
        }
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;

    if let Ok(mut log) = log.lock() {
        log.push(format!("{method} {path}"));
    }
    let (status, response) = route(&method, &path, &auth, &body);
    let response = response.to_string();
    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
        if status == 200 { "OK" } else { "Error" },
        response.len()
    )
}

fn route(method: &str, path: &str, auth: &str, body: &[u8]) -> (u16, Value) {
    if auth != format!("Key {MOCK_API_KEY}") {
        return (403, json!({ "message": "Invalid API key" }));
    }
    match (method, path) {
        ("GET", "/api/data_sources") => (
            200,
            json!([
                { "id": 1, "name": "Analytics DB", "type": "pg" },
                { "id": 2, "name": "Events", "type": "clickhouse" },
            ]),
        ),
        ("POST", "/api/query_results") => {
            let sql = serde_json::from_slice::<Value>(body)
                .ok()
                .and_then(|v| v.get("query").and_then(Value::as_str).map(str::to_owned))
                .unwrap_or_default();
            let id = if sql.contains("fail") { "failing" } else { "ok" };
            (200, json!({ "job": { "id": id, "status": 1, "error": "", "query_result_id": null } }))
        }
        ("GET", "/api/jobs/ok") => {
            (200, json!({ "job": { "id": "ok", "status": 3, "error": "", "query_result_id": 9 } }))
        }
        ("GET", "/api/jobs/failing") => (
            200,
            json!({ "job": {
                "id": "failing",
                "status": 4,
                "error": "syntax error at or near \"fail\"",
                "query_result_id": null,
            } }),
        ),
        ("GET", "/api/query_results/9") => (200, sample_result()),
        _ => (404, json!({ "message": "not found" })),
    }
}

fn sample_result() -> Value {
    let rows: Vec<Value> = (1..=40)
        .map(|i| {
            let plan = ["free", "pro", "team"][i % 3];
            json!({
                "id": i,
                "email": format!("user{i}@example.com"),
                "plan": plan,
                "signed_up": format!("2026-09-{:02}", i % 28 + 1),
                "mrr": if i % 3 == 0 { Value::Null } else { json!(i as f64 * 12.5) },
            })
        })
        .collect();
    let columns: Vec<Value> =
        ["id", "email", "plan", "signed_up", "mrr"].iter().map(|c| json!({ "name": c })).collect();
    json!({ "query_result": { "runtime": 0.137, "data": { "columns": columns, "rows": rows } } })
}
