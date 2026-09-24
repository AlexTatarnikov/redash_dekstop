//! Saved queries: snapshots of the editor's query (SQL, data source and variable
//! definitions) the user keeps on purpose, like history but without a limit and with
//! a name they can change. Pure: creating and renaming. Newest first.

use serde::{Deserialize, Serialize};

use crate::history::Entry;
use crate::vars::Variable;

/// The name of a query whose SQL has no text to name it after.
pub const UNTITLED: &str = "Untitled query";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedQuery {
    /// Shown in the list; the first SQL line until the user renames it.
    pub name: String,
    /// Unix time, in seconds.
    pub saved_at: u64,
    pub data_source_id: i64,
    /// As written, with `{{ name }}` references.
    pub sql: String,
    /// Definitions only (no ids or run results).
    pub variables: Vec<Variable>,
}

impl SavedQuery {
    pub fn new(saved_at: u64, data_source_id: i64, sql: &str, variables: &[Variable]) -> Self {
        let snapshot = Entry::new(saved_at, data_source_id, sql, variables);
        let name = match snapshot.title() {
            "" => UNTITLED.to_string(),
            title => title.to_string(),
        };
        Self { name, saved_at, data_source_id, sql: snapshot.sql, variables: snapshot.variables }
    }
}

/// Renames the query at `index` to `name`, trimmed. A blank name keeps the old one.
/// Returns whether anything changed.
pub fn rename(saved: &mut [SavedQuery], index: usize, name: &str) -> bool {
    let name = name.trim();
    match saved.get_mut(index) {
        Some(query) if !name.is_empty() && query.name != name => {
            query.name = name.to_string();
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vars::{Definition, Run};

    #[test]
    fn named_after_the_first_sql_line() {
        assert_eq!(SavedQuery::new(0, 1, "-- Paying users\nSELECT id", &[]).name, "SELECT id");
        assert_eq!(SavedQuery::new(0, 1, "  \n", &[]).name, UNTITLED);
    }

    #[test]
    fn snapshots_variable_definitions_only() {
        let var = Variable {
            id: 3,
            name: "n".into(),
            def: Definition::Value { value: "1".into() },
            run: Some(Run::Failed("x".into())),
        };
        let q = SavedQuery::new(5, 2, "select {{ n }}", std::slice::from_ref(&var));
        assert_eq!(q.variables, [Variable { id: 0, run: None, ..var }]);
        assert_eq!((q.saved_at, q.data_source_id, q.sql.as_str()), (5, 2, "select {{ n }}"));
    }

    #[test]
    fn renames_with_a_trimmed_non_blank_name() {
        let mut saved = vec![SavedQuery::new(0, 1, "select 1", &[])];
        assert!(rename(&mut saved, 0, "  Paying users "));
        assert_eq!(saved[0].name, "Paying users");
        assert!(!rename(&mut saved, 0, "   "), "blank keeps the name");
        assert!(!rename(&mut saved, 0, "Paying users"), "unchanged");
        assert!(!rename(&mut saved, 3, "x"), "no such query");
        assert_eq!(saved[0].name, "Paying users");
    }
}
