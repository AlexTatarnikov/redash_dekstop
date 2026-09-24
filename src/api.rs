//! Minimal blocking client for the Redash REST API.
//! Calls are made from background threads so the UI never blocks.

use std::thread;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
    base: String,
    api_key: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DataSource {
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Column {
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct QueryData {
    pub columns: Vec<Column>,
    pub rows: Vec<serde_json::Map<String, Value>>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct QueryResult {
    pub data: QueryData,
    pub runtime: f64,
}

#[derive(Deserialize)]
struct QueryResultEnvelope {
    query_result: QueryResult,
}

#[derive(Deserialize)]
struct Job {
    id: String,
    status: u8,
    error: Option<String>,
    query_result_id: Option<i64>,
}

#[derive(Deserialize)]
struct JobEnvelope {
    job: Job,
}

// Redash job statuses.
const JOB_SUCCESS: u8 = 3;
const JOB_FAILURE: u8 = 4;
const JOB_CANCELLED: u8 = 5;

impl Client {
    pub fn new(host: &str, api_key: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(60)))
            .build()
            .into();
        let host = host.trim().trim_end_matches('/');
        let base = if host.starts_with("http://") || host.starts_with("https://") {
            host.to_string()
        } else {
            format!("https://{host}")
        };
        Self {
            agent,
            base,
            api_key: api_key.trim().to_string(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    fn auth(&self) -> String {
        format!("Key {}", self.api_key)
    }

    fn handle<T: DeserializeOwned>(mut resp: ureq::http::Response<ureq::Body>) -> Result<T> {
        let status = resp.status();
        let body = resp.body_mut().read_to_string()?;
        if !status.is_success() {
            // Redash usually returns {"message": "..."} on errors.
            let msg = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_owned))
                .unwrap_or(body);
            bail!("HTTP {status}: {msg}");
        }
        serde_json::from_str(&body).map_err(|e| anyhow!("unexpected response: {e}"))
    }

    fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = self
            .agent
            .get(format!("{}{path}", self.base))
            .header("Authorization", self.auth())
            .call()?;
        Self::handle(resp)
    }

    fn post<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T> {
        let resp = self
            .agent
            .post(format!("{}{path}", self.base))
            .header("Authorization", self.auth())
            .send_json(body)?;
        Self::handle(resp)
    }

    /// Also serves as a credentials check.
    pub fn data_sources(&self) -> Result<Vec<DataSource>> {
        self.get("/api/data_sources")
    }

    /// Runs an ad-hoc query and waits for its result.
    pub fn execute(&self, data_source_id: i64, sql: &str) -> Result<QueryResult> {
        let body = json!({
            "data_source_id": data_source_id,
            "query": sql,
            "max_age": 0,
            "parameters": {},
        });
        let resp: Value = self.post("/api/query_results", &body)?;
        if let Some(result) = resp.get("query_result") {
            return Ok(serde_json::from_value(result.clone())?);
        }
        let mut job = serde_json::from_value::<JobEnvelope>(resp)?.job;
        loop {
            match job.status {
                JOB_SUCCESS => {
                    let id = job
                        .query_result_id
                        .ok_or_else(|| anyhow!("job finished without a result"))?;
                    let env: QueryResultEnvelope = self.get(&format!("/api/query_results/{id}"))?;
                    return Ok(env.query_result);
                }
                JOB_FAILURE => bail!(job.error.unwrap_or_else(|| "query failed".into())),
                JOB_CANCELLED => bail!("query was cancelled"),
                _ => {
                    thread::sleep(Duration::from_millis(500));
                    job = self
                        .get::<JobEnvelope>(&format!("/api/jobs/{}", job.id))?
                        .job;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    /// Serves one canned JSON response per incoming request, in order,
    /// and returns the request lines it saw.
    fn mock_server(
        responses: Vec<(u16, &'static str)>,
    ) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let mut seen = Vec::new();
            for (status, body) in responses {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut len = 0;
                let mut auth = String::new();
                loop {
                    let mut h = String::new();
                    reader.read_line(&mut h).unwrap();
                    if h == "\r\n" {
                        break;
                    }
                    let lower = h.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    if lower.starts_with("authorization:") {
                        auth = h.trim().to_string();
                    }
                }
                reader
                    .by_ref()
                    .take(len)
                    .read_to_end(&mut Vec::new())
                    .unwrap();
                seen.push(format!("{} | {}", line.trim(), auth));
                let mut stream = stream;
                write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
            seen
        });
        (addr, handle)
    }

    #[test]
    fn execute_polls_job_until_result() {
        let (addr, server) = mock_server(vec![
            (
                200,
                r#"{"job":{"id":"abc","status":1,"error":"","query_result_id":null}}"#,
            ),
            (
                200,
                r#"{"job":{"id":"abc","status":3,"error":"","query_result_id":42}}"#,
            ),
            (
                200,
                r#"{"query_result":{"runtime":0.5,"data":{"columns":[{"name":"n","type":"integer"}],"rows":[{"n":1}]}}}"#,
            ),
        ]);
        let result = Client::new(&addr, "secret").execute(7, "select 1").unwrap();
        assert_eq!(result.data.columns[0].name, "n");
        assert_eq!(result.data.rows[0]["n"], 1);

        let seen = server.join().unwrap();
        assert!(seen[0].starts_with("POST /api/query_results "));
        assert!(seen[1].starts_with("GET /api/jobs/abc "));
        assert!(seen[2].starts_with("GET /api/query_results/42 "));
        assert!(seen.iter().all(|s| s.ends_with("Key secret")));
    }

    #[test]
    fn surfaces_job_failure_and_http_errors() {
        let (addr, _s) = mock_server(vec![(
            200,
            r#"{"job":{"id":"x","status":4,"error":"syntax error at or near \"selec\"","query_result_id":null}}"#,
        )]);
        let err = Client::new(&addr, "k").execute(1, "selec").unwrap_err();
        assert!(err.to_string().contains("syntax error"));

        let (addr, _s) = mock_server(vec![(403, r#"{"message":"Invalid API key"}"#)]);
        let err = Client::new(&addr, "bad").data_sources().unwrap_err();
        assert_eq!(err.to_string(), "HTTP 403 Forbidden: Invalid API key");
    }

    #[test]
    fn normalizes_host() {
        assert_eq!(
            Client::new("redash.example.com/", "k").base_url(),
            "https://redash.example.com"
        );
        assert_eq!(
            Client::new(" http://localhost:5000 ", "k").base_url(),
            "http://localhost:5000"
        );
    }
}
