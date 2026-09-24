//! Pure formatting of query results for export: Markdown tables for the clipboard
//! and CSV files.

use serde_json::Value;

use crate::api::QueryResult;

/// How a cell is shown: strings as-is, other JSON values in their JSON form, and
/// missing or null values as `NULL`.
pub fn cell_text(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "NULL".into(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// What kind of value a cell holds, for styling it in the results table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    Text,
    Null,
    Number,
    Boolean,
    /// A string starting with an ISO date, like `2026-09-24` or `2026-09-24T10:00:00Z`.
    Date,
    /// An array or object.
    Json,
}

impl ValueKind {
    pub fn of(v: Option<&Value>) -> Self {
        match v {
            None | Some(Value::Null) => Self::Null,
            Some(Value::Number(_)) => Self::Number,
            Some(Value::Bool(_)) => Self::Boolean,
            Some(Value::Array(_) | Value::Object(_)) => Self::Json,
            Some(Value::String(s)) if is_iso_date(s) => Self::Date,
            Some(Value::String(_)) => Self::Text,
        }
    }
}

/// `YYYY-MM-DD`, alone or followed by a time (`T` or a space).
fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    b.len() >= 10
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && matches!(b.get(10), None | Some(b'T' | b' '))
}

/// The given rows (indices) as a GitHub-flavoured Markdown table, one line per row.
/// Pipes are escaped and line breaks become `<br>` so each cell stays on its line.
pub fn markdown_table(r: &QueryResult, rows: &[usize]) -> String {
    let cell = |text: &str| text.replace('|', "\\|").replace("\r\n", "<br>").replace(['\n', '\r'], "<br>");
    let line = |cells: Vec<String>| format!("| {} |\n", cells.join(" | "));
    let columns = &r.data.columns;
    let mut out = line(columns.iter().map(|c| cell(&c.name)).collect());
    out += &line(columns.iter().map(|_| "---".to_string()).collect());
    for row in rows.iter().filter_map(|&i| r.data.rows.get(i)) {
        out += &line(columns.iter().map(|c| cell(&cell_text(row.get(&c.name)))).collect());
    }
    out
}

/// A query error as a fenced Markdown code block, fenced with more backticks than
/// any run inside it so the error can't close the block early.
pub fn markdown_error(error: &str) -> String {
    let longest = error.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}\n{}\n{fence}\n", error.trim_end())
}

/// All rows as CSV (RFC 4180, CRLF line endings, like Redash's own export).
/// Null values are empty fields.
pub fn csv(r: &QueryResult) -> String {
    let field = |text: &str| {
        if text.contains([',', '"', '\n', '\r']) {
            format!("\"{}\"", text.replace('"', "\"\""))
        } else {
            text.to_string()
        }
    };
    let line = |fields: Vec<String>| fields.join(",") + "\r\n";
    let columns = &r.data.columns;
    let mut out = line(columns.iter().map(|c| field(&c.name)).collect());
    for row in &r.data.rows {
        out += &line(
            columns
                .iter()
                .map(|c| match row.get(&c.name) {
                    None | Some(Value::Null) => String::new(),
                    value => field(&cell_text(value)),
                })
                .collect(),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::api::{Column, QueryData};

    #[test]
    fn value_kinds() {
        let kind = |v: Value| ValueKind::of(Some(&v));
        assert_eq!(ValueKind::of(None), ValueKind::Null);
        assert_eq!(kind(json!(null)), ValueKind::Null);
        assert_eq!(kind(json!(12.5)), ValueKind::Number);
        assert_eq!(kind(json!(true)), ValueKind::Boolean);
        assert_eq!(kind(json!([1])), ValueKind::Json);
        assert_eq!(kind(json!({"a": 1})), ValueKind::Json);
        assert_eq!(kind(json!("2026-09-02")), ValueKind::Date);
        assert_eq!(kind(json!("2026-09-02T10:00:00Z")), ValueKind::Date);
        assert_eq!(kind(json!("2026-09-02 10:00")), ValueKind::Date);
        assert_eq!(kind(json!("2026-09-02x")), ValueKind::Text);
        assert_eq!(kind(json!("12")), ValueKind::Text);
        assert_eq!(kind(json!("pro")), ValueKind::Text);
    }

    fn result() -> QueryResult {
        let rows = [
            json!({"id": 1, "name": "plain", "note": null, "tags": ["a", "b"]}),
            json!({"id": 2, "name": "a|b, \"c\"", "note": "line1\nline2"}),
            json!({"id": 3.5, "name": "", "note": true}),
        ];
        QueryResult {
            data: QueryData {
                columns: ["id", "name", "note", "tags"].map(|n| Column { name: n.into() }).to_vec(),
                rows: rows.into_iter().filter_map(|r| r.as_object().cloned()).collect(),
            },
            runtime: 0.1,
        }
    }

    #[test]
    fn markdown_table_of_selected_rows() {
        assert_eq!(
            markdown_table(&result(), &[1, 2]),
            "| id | name | note | tags |\n\
             | --- | --- | --- | --- |\n\
             | 2 | a\\|b, \"c\" | line1<br>line2 | NULL |\n\
             | 3.5 |  | true | NULL |\n"
        );
    }

    #[test]
    fn markdown_table_without_rows_keeps_header() {
        assert_eq!(
            markdown_table(&result(), &[]),
            "| id | name | note | tags |\n| --- | --- | --- | --- |\n"
        );
    }

    #[test]
    fn markdown_error_is_a_code_block_that_contains_its_backticks() {
        assert_eq!(markdown_error("syntax error\n"), "```\nsyntax error\n```\n");
        assert_eq!(markdown_error("bad ```x``` here"), "````\nbad ```x``` here\n````\n");
    }

    #[test]
    fn csv_quotes_special_fields_and_leaves_nulls_empty() {
        assert_eq!(
            csv(&result()),
            "id,name,note,tags\r\n\
             1,plain,,\"[\"\"a\"\",\"\"b\"\"]\"\r\n\
             2,\"a|b, \"\"c\"\"\",\"line1\nline2\",\r\n\
             3.5,,true,\r\n"
        );
    }
}
