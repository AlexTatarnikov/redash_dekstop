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

/// A table (or view) in a data source's schema, as Redash lists it: `users` for
/// Postgres' `public` schema, otherwise usually `schema.table`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Table {
    pub name: String,
    pub columns: Vec<TableColumn>,
}

/// Older Redash versions list columns as bare names, newer ones as `{name, type}`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(from = "ColumnRepr")]
pub struct TableColumn {
    pub name: String,
    pub kind: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ColumnRepr {
    Name(String),
    Typed {
        name: String,
        #[serde(rename = "type")]
        kind: Option<String>,
    },
}

impl From<ColumnRepr> for TableColumn {
    fn from(repr: ColumnRepr) -> Self {
        match repr {
            ColumnRepr::Name(name) => Self { name, kind: None },
            ColumnRepr::Typed { name, kind } => Self { name, kind },
        }
    }
}

#[derive(Deserialize)]
struct Job {
    id: String,
    status: u8,
    /// A message, or `{code, message}` for schema jobs.
    #[serde(default)]
    error: Value,
    query_result_id: Option<i64>,
    /// What a schema job produced.
    #[serde(default)]
    result: Value,
}

/// The message of a job error: a string, or `{code, message}`.
fn error_message(error: &Value) -> Option<String> {
    let msg = match error {
        Value::Object(e) => e.get("message").and_then(Value::as_str),
        e => e.as_str(),
    };
    msg.filter(|m| !m.is_empty()).map(str::to_owned)
}

#[derive(Deserialize)]
struct JobEnvelope {
    job: Job,
}

// Redash job statuses.
const JOB_SUCCESS: u8 = 3;
const JOB_FAILURE: u8 = 4;
const JOB_CANCELLED: u8 = 5;
/// Redash's error code for query runners that can't list their schema.
const SCHEMA_NOT_SUPPORTED: u64 = 1;

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
        let job = self.finish(serde_json::from_value::<JobEnvelope>(resp)?.job)?;
        match job.status {
            JOB_SUCCESS => {
                let id = job.query_result_id.ok_or_else(|| anyhow!("job finished without a result"))?;
                let env: QueryResultEnvelope = self.get(&format!("/api/query_results/{id}"))?;
                Ok(env.query_result)
            }
            JOB_FAILURE => bail!(error_message(&job.error).unwrap_or_else(|| "query failed".into())),
            _ => bail!("query was cancelled"),
        }
    }

    /// Tables and columns of a data source, for autocompletion. Redash answers from
    /// its cache or starts a refresh job; sources without schema support give none.
    pub fn schema(&self, data_source_id: i64) -> Result<Vec<Table>> {
        let resp: Value = self.get(&format!("/api/data_sources/{data_source_id}/schema"))?;
        if let Some(schema) = resp.get("schema") {
            return Ok(serde_json::from_value(schema.clone())?);
        }
        if let Some(error) = resp.get("error") {
            return schema_failure(error);
        }
        let job = self.finish(serde_json::from_value::<JobEnvelope>(resp)?.job)?;
        match job.status {
            JOB_SUCCESS if job.result.is_null() => Ok(Vec::new()),
            JOB_SUCCESS => Ok(serde_json::from_value(job.result)?),
            JOB_FAILURE => schema_failure(&job.error),
            _ => bail!("schema refresh was cancelled"),
        }
    }

    /// Polls `job` until it succeeds, fails or is cancelled.
    fn finish(&self, mut job: Job) -> Result<Job> {
        while !matches!(job.status, JOB_SUCCESS | JOB_FAILURE | JOB_CANCELLED) {
            thread::sleep(Duration::from_millis(500));
            job = self.get::<JobEnvelope>(&format!("/api/jobs/{}", job.id))?.job;
        }
        Ok(job)
    }
}

fn schema_failure(error: &Value) -> Result<Vec<Table>> {
    if error.get("code").and_then(Value::as_u64) == Some(SCHEMA_NOT_SUPPORTED) {
        return Ok(Vec::new());
    }
    bail!(error_message(error).unwrap_or_else(|| "could not load the schema".into()))
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

        let err = client(&mock, MOCK_API_KEY).execute(1, "-- select 1").unwrap_err();
        assert_eq!(err.to_string(), "can't execute an empty query");

        let err = client(&mock, "bad").data_sources().unwrap_err();
        assert_eq!(err.to_string(), "HTTP 403 Forbidden: Invalid API key");
    }

    #[test]
    fn schema_from_cache_or_refresh_job() {
        let mock = MockRedash::start().unwrap();
        let tables = client(&mock, MOCK_API_KEY).schema(1).unwrap();
        let users = tables.iter().find(|t| t.name == "users").unwrap();
        assert_eq!(users.columns[0], TableColumn { name: "id".into(), kind: Some("integer".into()) });

        let tables = client(&mock, MOCK_API_KEY).schema(2).unwrap();
        assert_eq!(tables[0].name, "events");
        assert_eq!(tables[0].columns[0], TableColumn { name: "event_id".into(), kind: None });
        assert!(
            mock.requests()
                .ends_with(&["GET /api/data_sources/2/schema".into(), "GET /api/jobs/schema".into()])
        );

        assert_eq!(client(&mock, MOCK_API_KEY).schema(3).unwrap(), [], "schema not supported");
    }

    #[test]
    fn normalizes_host() {
        assert_eq!(normalize_host("redash.example.com/"), "https://redash.example.com");
        assert_eq!(normalize_host(" http://localhost:5000 "), "http://localhost:5000");
    }
}
