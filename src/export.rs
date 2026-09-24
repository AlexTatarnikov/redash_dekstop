//! Pure formatting of query results for export: Markdown tables for the clipboard
//! and CSV files.

use std::ops::Range;

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

/// The given rows as a GitHub-flavoured Markdown table, one line per row.
/// Pipes are escaped and line breaks become `<br>` so each cell stays on its line.
pub fn markdown_table(r: &QueryResult, rows: Range<usize>) -> String {
    let cell = |text: &str| text.replace('|', "\\|").replace("\r\n", "<br>").replace(['\n', '\r'], "<br>");
    let line = |cells: Vec<String>| format!("| {} |\n", cells.join(" | "));
    let columns = &r.data.columns;
    let mut out = line(columns.iter().map(|c| cell(&c.name)).collect());
    out += &line(columns.iter().map(|_| "---".to_string()).collect());
    for row in &r.data.rows[rows] {
        out += &line(columns.iter().map(|c| cell(&cell_text(row.get(&c.name)))).collect());
    }
    out
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
            markdown_table(&result(), 1..3),
            "| id | name | note | tags |\n\
             | --- | --- | --- | --- |\n\
             | 2 | a\\|b, \"c\" | line1<br>line2 | NULL |\n\
             | 3.5 |  | true | NULL |\n"
        );
    }

    #[test]
    fn markdown_table_without_rows_keeps_header() {
        assert_eq!(
            markdown_table(&result(), 0..0),
            "| id | name | note | tags |\n| --- | --- | --- | --- |\n"
        );
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
