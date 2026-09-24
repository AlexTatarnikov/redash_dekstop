//! Pure schema browsing: which tables and columns the schema panel shows for a filter.

use crate::api::{Table, TableColumn};

/// A table shown in the schema panel, with the columns to list under it.
#[derive(Debug, PartialEq)]
pub struct Match<'a> {
    pub table: &'a Table,
    pub columns: Vec<&'a TableColumn>,
    /// Shown for some of its columns rather than its name; the panel expands these.
    pub by_column: bool,
}

/// The tables matching `filter` (case-insensitive substring): tables matching by name
/// first, then those matching by column, each group in schema order. A table whose
/// name matches keeps all its columns; otherwise only the matching columns are kept,
/// and tables with none are left out. An empty filter keeps all.
pub fn filter<'a>(tables: &'a [Table], filter: &str) -> Vec<Match<'a>> {
    let needle = filter.trim().to_lowercase();
    let matches = |name: &str| name.to_lowercase().contains(&needle);
    let mut shown: Vec<_> = tables
        .iter()
        .filter_map(|table| {
            if needle.is_empty() || matches(&table.name) {
                return Some(Match { table, columns: table.columns.iter().collect(), by_column: false });
            }
            let columns: Vec<_> = table.columns.iter().filter(|c| matches(&c.name)).collect();
            (!columns.is_empty()).then_some(Match { table, columns, by_column: true })
        })
        .collect();
    // Stable: keeps schema order within each group.
    shown.sort_by_key(|m| m.by_column);
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> Vec<Table> {
        let table = |name: &str, columns: &[&str]| Table {
            name: name.into(),
            columns: columns.iter().map(|c| TableColumn { name: (*c).into(), kind: None }).collect(),
        };
        vec![
            table("users", &["id", "email", "plan"]),
            table("orders", &["id", "user_id", "amount"]),
            table("billing.invoices", &["id", "order_id"]),
            table("plans", &["id", "price"]),
        ]
    }

    fn shown(filter_text: &str) -> Vec<(String, Vec<String>, bool)> {
        let tables = tables();
        filter(&tables, filter_text)
            .into_iter()
            .map(|m| {
                let columns = m.columns.iter().map(|c| c.name.clone()).collect();
                (m.table.name.clone(), columns, m.by_column)
            })
            .collect()
    }

    #[test]
    fn empty_filter_shows_everything() {
        let all = shown("  ");
        assert_eq!(all.len(), 4);
        assert_eq!(all[0], ("users".into(), vec!["id".into(), "email".into(), "plan".into()], false));
    }

    #[test]
    fn table_name_match_keeps_all_columns() {
        assert_eq!(
            shown("BILLING"),
            [("billing.invoices".into(), vec!["id".into(), "order_id".into()], false)]
        );
    }

    #[test]
    fn column_match_keeps_only_matching_columns() {
        assert_eq!(
            shown("user"),
            [
                ("users".into(), vec!["id".into(), "email".into(), "plan".into()], false),
                ("orders".into(), vec!["user_id".into()], true),
            ]
        );
        assert_eq!(shown("nothing"), []);
    }

    #[test]
    fn table_name_matches_come_before_column_matches() {
        assert_eq!(
            shown("plan"),
            [
                ("plans".into(), vec!["id".into(), "price".into()], false),
                ("users".into(), vec!["plan".into()], true),
            ]
        );
    }
}
