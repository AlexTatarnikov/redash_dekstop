//! In-process fake Redash server, used by tests and by `cargo run -- --mock`.
//!
//! Behaviour (keep AGENTS.md in sync when changing it):
//! - any request without `Authorization: Key test-key` → 403 "Invalid API key"
//! - `GET /api/data_sources` → "Analytics DB" (id 1) and "Events" (id 2)
//! - `POST /api/query_results` → a pending job (its SQL is kept for `queries()`); SQL containing `fail` makes the job fail,
//!   and SQL that is only comments fails like Postgres does ("can't execute an empty query")
//! - `GET /api/jobs/{id}` → finished job pointing at result 9 (or the failure)
//! - `GET /api/query_results/9` → 40 rows of `id, email, plan, signed_up, mrr`
//! - `GET /api/data_sources/1/schema` → cached schema: `users`, `orders`, `billing.invoices`
//!   (columns with types); source 2 → a refresh job, whose `GET /api/jobs/schema` gives
//!   `events` (bare column names); any other source → "schema not supported"

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{Value, json};

use crate::state::has_statement;

pub const MOCK_API_KEY: &str = "test-key";

pub struct MockRedash {
    url: String,
    log: Arc<Log>,
}

#[derive(Default)]
struct Log {
    requests: Mutex<Vec<String>>,
    queries: Mutex<Vec<String>>,
}

impl MockRedash {
    /// Starts the server on a free localhost port. It runs until the process exits.
    pub fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let url = format!("http://{}", listener.local_addr()?);
        let log = Arc::new(Log::default());
        let server_log = Arc::clone(&log);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                // A malformed request only affects that connection.
                let _ = handle(stream, &server_log);
            }
        });
        Ok(Self { url, log })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Requests received so far, as `"METHOD /path"`.
    pub fn requests(&self) -> Vec<String> {
        self.log.requests.lock().map(|r| r.clone()).unwrap_or_default()
    }

    /// SQL of the queries run so far, in order.
    pub fn queries(&self) -> Vec<String> {
        self.log.queries.lock().map(|q| q.clone()).unwrap_or_default()
    }
}

fn handle(stream: TcpStream, log: &Log) -> std::io::Result<()> {
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

    if let Ok(mut requests) = log.requests.lock() {
        requests.push(format!("{method} {path}"));
    }
    if (method.as_str(), path.as_str()) == ("POST", "/api/query_results")
        && let Some(sql) =
            serde_json::from_slice::<Value>(&body).ok().and_then(|v| v["query"].as_str().map(str::to_owned))
        && let Ok(mut queries) = log.queries.lock()
    {
        queries.push(sql);
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
            let id = if !has_statement(&sql) {
                "empty"
            } else if sql.contains("fail") {
                "failing"
            } else {
                "ok"
            };
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
        ("GET", "/api/jobs/empty") => (
            200,
            json!({ "job": {
                "id": "empty",
                "status": 4,
                "error": "can't execute an empty query",
                "query_result_id": null,
            } }),
        ),
        ("GET", "/api/query_results/9") => (200, sample_result()),
        ("GET", "/api/data_sources/1/schema") => (200, json!({ "schema": sample_schema() })),
        ("GET", "/api/data_sources/2/schema") => {
            (200, json!({ "job": { "id": "schema", "status": 1, "error": "", "result": null } }))
        }
        ("GET", "/api/jobs/schema") => (
            200,
            json!({ "job": {
                "id": "schema",
                "status": 3,
                "error": "",
                "result": [{ "name": "events", "columns": ["event_id", "user_id", "name", "ts"] }],
            } }),
        ),
        ("GET", p) if p.starts_with("/api/data_sources/") && p.ends_with("/schema") => (
            200,
            json!({ "error": { "code": 1, "message": "Data source type does not support retrieving schema" } }),
        ),
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

fn sample_schema() -> Value {
    let table = |name: &str, columns: &[(&str, &str)]| {
        let columns: Vec<Value> = columns.iter().map(|(n, t)| json!({ "name": n, "type": t })).collect();
        json!({ "name": name, "columns": columns })
    };
    json!([
        table(
            "users",
            &[
                ("id", "integer"),
                ("email", "text"),
                ("plan", "text"),
                ("signed_up", "date"),
                ("mrr", "numeric"),
                ("deleted", "boolean"),
            ],
        ),
        table(
            "orders",
            &[("id", "integer"), ("user_id", "integer"), ("amount", "numeric"), ("created_at", "timestamp")]
        ),
        table("billing.invoices", &[("id", "integer"), ("order_id", "integer"), ("paid_at", "timestamp")]),
    ])
}
