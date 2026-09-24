//! Execution history, like Postman's: every run of the editor's query records a
//! snapshot of the SQL, data source and variable definitions, so it can be restored
//! later. Pure: recording, titles and relative times. Newest first, at most [`LIMIT`].

use serde::{Deserialize, Serialize};

use crate::vars::Variable;

/// Entries kept; older ones are dropped.
pub const LIMIT: usize = 20;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Unix time, in seconds.
    pub executed_at: u64,
    pub data_source_id: i64,
    /// As written, with `{{ name }}` references.
    pub sql: String,
    /// Definitions only (no ids or run results).
    pub variables: Vec<Variable>,
}

impl Entry {
    pub fn new(executed_at: u64, data_source_id: i64, sql: &str, variables: &[Variable]) -> Self {
        let variables = variables
            .iter()
            .map(|v| Variable { id: 0, name: v.name.clone(), def: v.def.clone(), run: None })
            .collect();
        Self { executed_at, data_source_id, sql: sql.to_string(), variables }
    }

    /// The first line with SQL on it (skipping blank and `--` lines), trimmed.
    pub fn title(&self) -> &str {
        let mut lines = self.sql.lines().map(str::trim).filter(|l| !l.is_empty());
        let first = lines.clone().next().unwrap_or_default();
        lines.find(|l| !l.starts_with("--")).unwrap_or(first)
    }
}

/// Adds `entry` as the newest. Rerunning exactly the newest snapshot only updates
/// its time, so repeated runs don't push everything else out.
pub fn record(history: &mut Vec<Entry>, entry: Entry) {
    if let Some(newest) = history.first_mut()
        && (newest.data_source_id, &newest.sql, &newest.variables)
            == (entry.data_source_id, &entry.sql, &entry.variables)
    {
        newest.executed_at = entry.executed_at;
        return;
    }
    history.insert(0, entry);
    history.truncate(LIMIT);
}

/// How long before `now` something happened: "just now", "5 min ago", "3 h ago", "2 d ago".
pub fn ago(then: u64, now: u64) -> String {
    let secs = now.saturating_sub(then);
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86400 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86400),
    }
}

/// The current Unix time in seconds; 0 if the clock is before 1970.
pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vars::{Definition, Run};

    fn entry(at: u64, sql: &str) -> Entry {
        Entry::new(at, 1, sql, &[])
    }

    #[test]
    fn keeps_newest_first_up_to_the_limit() {
        let mut history = Vec::new();
        for i in 0..25 {
            record(&mut history, entry(i, &format!("select {i}")));
        }
        assert_eq!(history.len(), LIMIT);
        assert_eq!(history[0].sql, "select 24");
        assert_eq!(history[LIMIT - 1].sql, "select 5");
    }

    #[test]
    fn rerunning_the_newest_only_updates_its_time() {
        let mut history = vec![entry(1, "select 1")];
        record(&mut history, entry(5, "select 1"));
        assert_eq!(history, [entry(5, "select 1")]);
        record(&mut history, Entry::new(6, 2, "select 1", &[]));
        assert_eq!(history.len(), 2, "another data source is another entry");
        record(&mut history, entry(7, "select 1"));
        assert_eq!(history.len(), 3, "only the newest is merged");
    }

    #[test]
    fn snapshots_variable_definitions_only() {
        let var = Variable {
            id: 3,
            name: "ids".into(),
            def: Definition::Query { data_source_id: 1, sql: "select 1".into() },
            run: Some(Run::Failed("x".into())),
        };
        let e = Entry::new(0, 1, "select {{ ids }}", std::slice::from_ref(&var));
        assert_eq!(e.variables, [Variable { id: 0, run: None, ..var }]);
    }

    #[test]
    fn title_is_first_sql_line() {
        assert_eq!(entry(0, "\n-- Paying users\n  SELECT id\nFROM users").title(), "SELECT id");
        assert_eq!(entry(0, "-- only a comment").title(), "-- only a comment");
        assert_eq!(entry(0, "").title(), "");
    }

    #[test]
    fn relative_times() {
        assert_eq!(ago(100, 159), "just now");
        assert_eq!(ago(0, 60 * 5 + 7), "5 min ago");
        assert_eq!(ago(0, 3600 * 3), "3 h ago");
        assert_eq!(ago(0, 86400 * 2), "2 d ago");
        assert_eq!(ago(10, 0), "just now", "clock went back");
    }
}
