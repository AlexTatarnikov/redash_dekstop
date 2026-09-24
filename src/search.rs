//! Pure result search, like a browser's find in page: every occurrence of the query
//! in the values of the shown page of a result, and the rows that contain one. Only
//! the page is searched, so huge results stay cheap.

use std::ops::Range;

use crate::api::QueryResult;
use crate::export::cell_text;

/// One occurrence of the query: in which cell, and where in its text as shown.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub row: usize,
    pub column: usize,
    /// Byte range in the cell's [`cell_text`].
    pub range: Range<usize>,
}

/// What a search found.
#[derive(Debug, Default, PartialEq)]
pub struct Found {
    /// Indices of the rows to show: those searched with a hit, or all of them for a
    /// blank query.
    pub rows: Vec<usize>,
    /// Every occurrence, in reading order: by row, then column, then position.
    pub hits: Vec<Hit>,
}

/// Finds `query` (case-insensitive) in the given rows' values as the table shows them,
/// so `null` finds NULLs. A blank query finds nothing and keeps all the rows.
pub fn search(r: &QueryResult, query: &str, rows: Range<usize>) -> Found {
    let rows = rows.start.min(r.data.rows.len())..rows.end.min(r.data.rows.len());
    if query.trim().is_empty() {
        return Found { rows: rows.collect(), hits: Vec::new() };
    }
    let needle = query.to_lowercase();
    let needle_chars: Vec<char> = needle.chars().collect();
    let mut found = Found::default();
    for (row, values) in rows.clone().zip(&r.data.rows[rows]) {
        let before = found.hits.len();
        for (column, c) in r.data.columns.iter().enumerate() {
            let text = cell_text(values.get(&c.name));
            if !text.to_lowercase().contains(&needle) {
                continue;
            }
            let ranges = find_all(&text, &needle_chars);
            found.hits.extend(ranges.into_iter().map(|range| Hit { row, column, range }));
        }
        if found.hits.len() > before {
            found.rows.push(row);
        }
    }
    found
}

/// Byte ranges of the non-overlapping occurrences of `needle` (lowercase) in `text`,
/// ignoring case.
fn find_all(text: &str, needle: &[char]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    while let Some(c) = text[start..].chars().next() {
        match match_len(&text[start..], needle) {
            Some(len) => {
                out.push(start..start + len);
                start += len;
            }
            None => start += c.len_utf8(),
        }
    }
    out
}

/// Length in bytes of the start of `text` that equals `needle` (lowercase), ignoring case.
fn match_len(text: &str, needle: &[char]) -> Option<usize> {
    let mut want = needle.iter();
    let mut len = 0;
    let mut chars = text.chars();
    while !want.as_slice().is_empty() {
        let c = chars.next()?;
        for lower in c.to_lowercase() {
            if want.next() != Some(&lower) {
                return None;
            }
        }
        len += c.len_utf8();
    }
    Some(len)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::api::{Column, QueryData};

    fn result() -> QueryResult {
        let rows = [
            json!({"id": 1, "email": "ann@example.com", "plan": "Pro", "mrr": null}),
            json!({"id": 2, "email": "bob@example.com", "plan": "free", "mrr": 12.5}),
            json!({"id": 3, "email": "cy@propro.org", "plan": "pro", "mrr": 30}),
        ];
        QueryResult {
            data: QueryData {
                columns: ["id", "email", "plan", "mrr"].map(|n| Column { name: n.into() }).to_vec(),
                rows: rows.into_iter().filter_map(|r| r.as_object().cloned()).collect(),
            },
            runtime: 0.1,
        }
    }

    fn hit(row: usize, column: usize, range: Range<usize>) -> Hit {
        Hit { row, column, range }
    }

    #[test]
    fn blank_query_keeps_all_rows_and_finds_nothing() {
        assert_eq!(search(&result(), " ", 0..3), Found { rows: vec![0, 1, 2], hits: vec![] });
        assert_eq!(search(&result(), "", 1..9), Found { rows: vec![1, 2], hits: vec![] }, "clamped");
    }

    #[test]
    fn finds_every_occurrence_in_reading_order() {
        let found = search(&result(), "PRO", 0..3);
        assert_eq!(found.rows, [0, 2]);
        assert_eq!(found.hits, [hit(0, 2, 0..3), hit(2, 1, 3..6), hit(2, 1, 6..9), hit(2, 2, 0..3)]);
    }

    #[test]
    fn matches_values_as_shown_not_column_names() {
        assert_eq!(search(&result(), "12.5", 0..3).hits, [hit(1, 3, 0..4)]);
        assert_eq!(search(&result(), "null", 0..3).hits, [hit(0, 3, 0..4)]);
        assert_eq!(search(&result(), "email", 0..3), Found::default());
    }

    #[test]
    fn searches_only_the_given_rows() {
        let found = search(&result(), "pro", 1..3);
        assert_eq!(found.rows, [2]);
        assert_eq!(found.hits.len(), 3);
    }

    #[test]
    fn ranges_are_bytes_of_the_original_text() {
        let needle: Vec<char> = "é".chars().collect();
        assert_eq!(find_all("CAFÉ café", &needle), [3..5, 9..11]);
        assert_eq!(find_all("aaaaa", &['a', 'a']), [0..2, 2..4], "non-overlapping");
    }
}
