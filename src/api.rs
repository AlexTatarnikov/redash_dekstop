//! Minimal blocking client for the Redash REST API.
//! Calls are made from background threads so the UI never blocks.

use std::thread;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::config::Config;

#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
    base: String,
    api_key: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct DataSource {
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Column {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct QueryData {
    pub columns: Vec<Column>,
    pub rows: Vec<serde_json::Map<String, Value>>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct QueryResult {
    pub data: QueryData,
    pub runtime: f64,
}

/// Trims the host and defaults to `https://` when no scheme is given.
pub fn normalize_host(host: &str) -> String {
    let host = host.trim().trim_end_matches('/');
    if host.starts_with("http://") || host.starts_with("https://") {
        host.to_string()
    } else {
        format!("https://{host}")
    }
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
    pub fn new(config: &Config) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(60)))
            .build()
            .into();
        Self { agent, base: normalize_host(&config.host), api_key: config.api_key.trim().to_string() }
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
        let resp =
            self.agent.get(format!("{}{path}", self.base)).header("Authorization", self.auth()).call()?;
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
                    let id = job.query_result_id.ok_or_else(|| anyhow!("job finished without a result"))?;
                    let env: QueryResultEnvelope = self.get(&format!("/api/query_results/{id}"))?;
                    return Ok(env.query_result);
                }
                JOB_FAILURE => bail!(job.error.unwrap_or_else(|| "query failed".into())),
                JOB_CANCELLED => bail!("query was cancelled"),
                _ => {
                    thread::sleep(Duration::from_millis(500));
                    job = self.get::<JobEnvelope>(&format!("/api/jobs/{}", job.id))?.job;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{MOCK_API_KEY, MockRedash};

    fn client(mock: &MockRedash, api_key: &str) -> Client {
        Client::new(&Config { host: mock.url().into(), api_key: api_key.into() })
    }

    #[test]
    fn execute_polls_job_until_result() {
        let mock = MockRedash::start().unwrap();
        let result = client(&mock, MOCK_API_KEY).execute(1, "select 1").unwrap();
        assert_eq!(result.data.columns[1].name, "email");
        assert_eq!(result.data.rows.len(), 40);
        assert_eq!(result.data.rows[0]["email"], "user1@example.com");
        assert_eq!(
            mock.requests(),
            ["POST /api/query_results", "GET /api/jobs/ok", "GET /api/query_results/9"]
        );
    }

    #[test]
    fn surfaces_job_failure_and_http_errors() {
        let mock = MockRedash::start().unwrap();
        let err = client(&mock, MOCK_API_KEY).execute(1, "select fail").unwrap_err();
        assert_eq!(err.to_string(), "syntax error at or near \"fail\"");

        let err = client(&mock, "bad").data_sources().unwrap_err();
        assert_eq!(err.to_string(), "HTTP 403 Forbidden: Invalid API key");
    }

    #[test]
    fn normalizes_host() {
        assert_eq!(normalize_host("redash.example.com/"), "https://redash.example.com");
        assert_eq!(normalize_host(" http://localhost:5000 "), "http://localhost:5000");
    }
}
