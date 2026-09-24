//! User-defined variables, referenced in SQL as `{{ name }}` (Redash's parameter
//! syntax). Pure: finding references, substituting values and turning a query
//! result into a value. Running variable queries is driven by `state.rs`.
//!
//! A variable is either a value pasted into the SQL as written (`'2026-01-01'`,
//! `42`, `users`), or a query whose result becomes the value: the first column as
//! SQL literals joined by `, `, so it fits `IN ({{ ids }})`; no rows give `NULL`.

use std::ops::Range;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::QueryResult;
use crate::sql::{Token, tokenize};

/// A variable as the user defined it; persisted between launches (without
/// `id` and `run`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Variable {
    /// Unique within a session, so answers find their variable after renames.
    #[serde(skip)]
    pub id: u64,
    pub name: String,
    #[serde(flatten)]
    pub def: Definition,
    /// The last run of a query variable.
    #[serde(skip)]
    pub run: Option<Run>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Definition {
    /// Pasted into the SQL as written.
    Value { value: String },
    /// Run on `data_source_id`; its result becomes the value. May use other variables.
    Query { data_source_id: i64, sql: String },
}

/// What a query variable's run was made from: data source and SQL as written. A
/// result only counts while the definition still matches it.
pub type Source = (i64, String);

#[derive(Clone, Debug, PartialEq)]
pub enum Run {
    Running { ran: Source },
    Done { ran: Source, value: String, rows: usize },
    Failed(String),
}

impl Variable {
    /// The definition a run of this variable would use; `None` for plain values.
    pub fn source(&self) -> Option<Source> {
        match &self.def {
            Definition::Value { .. } => None,
            Definition::Query { data_source_id, sql } => Some((*data_source_id, sql.clone())),
        }
    }

    /// The value of a query variable if its last run matches its definition.
    pub fn fresh_value(&self) -> Option<&str> {
        match &self.run {
            Some(Run::Done { ran, value, .. }) if Some(ran) == self.source().as_ref() => Some(value),
            _ => None,
        }
    }
}

/// Whether `name` can be referenced: a letter or `_`, then letters, digits or `_`.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `{{ name }}` references in `sql` outside comments (inside strings too, as in
/// Redash), as the byte range of the whole reference and the trimmed name.
pub fn references(sql: &str) -> Vec<(Range<usize>, &str)> {
    let mut out = Vec::new();
    for (kind, range) in tokenize(sql) {
        if kind == Token::Comment {
            continue;
        }
        let mut from = range.start;
        while let Some(open) = sql[from..range.end].find("{{").map(|i| from + i) {
            let Some(close) = sql[open + 2..range.end].find("}}").map(|i| open + 2 + i) else {
                break;
            };
            let name = sql[open + 2..close].trim();
            if valid_name(name) {
                out.push((open..close + 2, name));
            }
            from = close + 2;
        }
    }
    out
}

/// `sql` with each reference replaced by `value(name)`.
pub fn substitute(sql: &str, value: impl Fn(&str) -> String) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut last = 0;
    for (range, name) in references(sql) {
        out.push_str(&sql[last..range.start]);
        out.push_str(&value(name));
        last = range.end;
    }
    out.push_str(&sql[last..]);
    out
}

/// The value of a query variable: its first column as SQL literals, joined by `, `,
/// and `NULL` when there are no rows.
pub fn value_of(result: &QueryResult) -> String {
    let Some(column) = result.data.columns.first() else {
        return "NULL".into();
    };
    let values: Vec<String> = result.data.rows.iter().map(|row| literal(row.get(&column.name))).collect();
    if values.is_empty() { "NULL".into() } else { values.join(", ") }
}

/// A JSON cell as an SQL literal: strings (and arrays or objects, as JSON) quoted.
fn literal(v: Option<&Value>) -> String {
    let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
    match v {
        None | Some(Value::Null) => "NULL".into(),
        Some(Value::Bool(b)) => if *b { "TRUE" } else { "FALSE" }.into(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => quote(s),
        Some(other) => quote(&other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::api::{Column, QueryData};

    fn names(sql: &str) -> Vec<&str> {
        references(sql).into_iter().map(|(_, name)| name).collect()
    }

    #[test]
    fn finds_references_outside_comments() {
        assert_eq!(
            names(
                "SELECT {{a}}, '{{ b }}' -- {{ c }}\n/* {{d}} */ FROM {{  e_1 }} {{ not valid }} {{1x}} {{f"
            ),
            ["a", "b", "e_1"]
        );
        assert_eq!(names("x = '{{a}} and {{ b }}'"), ["a", "b"], "several in one string");
    }

    #[test]
    fn substitutes_references() {
        let sql = "WHERE d > '{{ start }}' AND id IN ({{ids}}) -- {{ start }}";
        let out = substitute(sql, |name| if name == "start" { "2026-01-01".into() } else { "1, 2".into() });
        assert_eq!(out, "WHERE d > '2026-01-01' AND id IN (1, 2) -- {{ start }}");
    }

    #[test]
    fn validates_names() {
        assert!(valid_name("start_date") && valid_name("_x1"));
        assert!(!valid_name("") && !valid_name("1x") && !valid_name("a b") && !valid_name("ё"));
    }

    fn result(columns: &[&str], rows: Vec<Value>) -> QueryResult {
        QueryResult {
            data: QueryData {
                columns: columns.iter().map(|c| Column { name: c.to_string() }).collect(),
                rows: rows.into_iter().filter_map(|r| r.as_object().cloned()).collect(),
            },
            runtime: 0.1,
        }
    }

    #[test]
    fn value_is_first_column_as_literals() {
        let r = result(
            &["v", "other"],
            vec![
                json!({"v": 1, "other": "x"}),
                json!({"v": 2.5}),
                json!({"v": "it's"}),
                json!({"v": null}),
                json!({"v": true}),
                json!({"v": [1]}),
                json!({}),
            ],
        );
        assert_eq!(value_of(&r), "1, 2.5, 'it''s', NULL, TRUE, '[1]', NULL");
    }

    #[test]
    fn empty_result_is_null() {
        assert_eq!(value_of(&result(&["v"], vec![])), "NULL");
        assert_eq!(value_of(&result(&[], vec![])), "NULL");
    }

    #[test]
    fn persists_definitions_only() {
        let var = Variable {
            id: 7,
            name: "ids".into(),
            def: Definition::Query { data_source_id: 2, sql: "select 1".into() },
            run: Some(Run::Failed("x".into())),
        };
        let json = serde_json::to_value(&var).unwrap();
        assert_eq!(json, json!({"name": "ids", "kind": "query", "data_source_id": 2, "sql": "select 1"}));
        let back: Variable = serde_json::from_value(json).unwrap();
        assert_eq!((back.id, back.run, back.def), (0, None, var.def));
    }

    #[test]
    fn value_is_fresh_only_for_current_definition() {
        let mut var = Variable {
            id: 1,
            name: "ids".into(),
            def: Definition::Query { data_source_id: 1, sql: "select 1".into() },
            run: Some(Run::Done { ran: (1, "select 1".into()), value: "1".into(), rows: 1 }),
        };
        assert_eq!(var.fresh_value(), Some("1"));
        var.def = Definition::Query { data_source_id: 2, sql: "select 1".into() };
        assert_eq!(var.fresh_value(), None);
    }
}
