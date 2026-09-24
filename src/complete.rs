//! SQL autocompletion from a data source's schema. Pure: the editor asks for the
//! suggestions at its cursor and draws them in a popup.
//!
//! - `alias.` / `table.` suggest that table's columns (aliases come from `FROM`/`JOIN`);
//!   `schema.` suggests the tables in that schema.
//! - After `FROM`, `JOIN`, `INTO`, `UPDATE` or `TABLE` only tables are suggested.
//! - Otherwise: columns of the tables the query mentions, then tables, then keywords,
//!   then (when no table is mentioned yet) columns of every table.

use std::ops::Range;

use crate::api::Table;
use crate::sql::{KEYWORDS, Token, is_word, tokenize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Column,
    Table,
    Keyword,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    /// What accepting inserts.
    pub text: String,
    pub kind: Kind,
    /// Shown next to the text: the column type, or what kind of name this is.
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Completion {
    /// Char range replaced on accept: the part of the word typed so far.
    pub replace: Range<usize>,
    pub items: Vec<Suggestion>,
}

const MAX_ITEMS: usize = 50;

/// Keywords followed by a table name.
const TABLE_CONTEXT: &[&str] = &["from", "join", "into", "update", "table"];

/// Suggestions for the word before `cursor` (a char index) in `sql`. Without
/// `forced` (Ctrl+Space), there must be something typed or a `qualifier.` first.
pub fn complete(sql: &str, cursor: usize, tables: &[Table], forced: bool) -> Option<Completion> {
    let at = sql.char_indices().nth(cursor).map_or(sql.len(), |(b, _)| b);
    if in_literal(sql, at) {
        return None;
    }
    let start = sql.as_bytes()[..at].iter().rposition(|&b| !is_word(b) && b != b'.').map_or(0, |i| i + 1);
    let word = &sql[start..at];
    if word.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let (qualifier, partial) = match word.rsplit_once('.') {
        Some(("", _)) => return None,
        Some((q, p)) => (Some(q), p),
        None => (None, word),
    };
    if qualifier.is_none() && partial.is_empty() && !forced {
        return None;
    }

    let mut items = match qualifier {
        Some(q) => qualified(sql, q, partial, tables),
        None => unqualified(sql, start, partial, tables),
    };
    // Nothing to complete when the word is already typed out.
    items.retain(|s| !s.text.eq_ignore_ascii_case(partial));
    items.truncate(MAX_ITEMS);
    if items.is_empty() {
        return None;
    }
    let end = sql[..at].chars().count();
    Some(Completion { replace: end - partial.chars().count()..end, items })
}

/// Replaces `replace` (chars) in `sql` with `text`; returns the new SQL and the
/// cursor position right after the inserted text.
pub fn apply(sql: &str, replace: &Range<usize>, text: &str) -> (String, usize) {
    let byte = |chars: usize| sql.char_indices().nth(chars).map_or(sql.len(), |(b, _)| b);
    let (start, end) = (byte(replace.start), byte(replace.end));
    let out = format!("{}{text}{}", &sql[..start], &sql[end..]);
    (out, replace.start + text.chars().count())
}

fn qualified(sql: &str, qualifier: &str, partial: &str, tables: &[Table]) -> Vec<Suggestion> {
    let aliased = references(sql, tables)
        .into_iter()
        .find(|(_, alias)| alias.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(qualifier)))
        .map(|(t, _)| t);
    if let Some(table) = aliased.or_else(|| find_table(tables, qualifier)) {
        return columns(&[table], partial);
    }
    // A schema name: suggest the tables in it.
    let schema = format!("{qualifier}.");
    tables
        .iter()
        .filter_map(|t| strip_prefix_ci(&t.name, &schema))
        .filter(|name| strip_prefix_ci(name, partial).is_some())
        .map(|name| Suggestion { text: name.into(), kind: Kind::Table, detail: "table".into() })
        .collect()
}

fn unqualified(sql: &str, start: usize, partial: &str, tables: &[Table]) -> Vec<Suggestion> {
    let wants_table = matches!(
        lex(&sql[..start]).last(),
        Some(Item::Word(w)) if TABLE_CONTEXT.iter().any(|k| w.eq_ignore_ascii_case(k))
    );
    let matching_tables = tables
        .iter()
        .filter(|t| {
            strip_prefix_ci(&t.name, partial).is_some() || strip_prefix_ci(short(&t.name), partial).is_some()
        })
        .map(|t| Suggestion { text: t.name.clone(), kind: Kind::Table, detail: "table".into() });
    if wants_table {
        return matching_tables.collect();
    }

    let mentioned: Vec<&Table> = references(sql, tables).into_iter().map(|(t, _)| t).collect();
    let mut items = columns(&mentioned, partial);
    items.extend(matching_tables);
    if !partial.is_empty() {
        let upper = partial.chars().any(char::is_uppercase);
        items.extend(KEYWORDS.iter().filter(|k| strip_prefix_ci(k, partial).is_some()).map(|k| Suggestion {
            text: if upper { k.to_uppercase() } else { k.to_string() },
            kind: Kind::Keyword,
            detail: "keyword".into(),
        }));
    }
    if mentioned.is_empty() {
        items.extend(columns(&tables.iter().collect::<Vec<_>>(), partial));
    }
    items
}

/// Columns of `tables` starting with `partial`, each name once.
fn columns(tables: &[&Table], partial: &str) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = Vec::new();
    for column in tables.iter().flat_map(|t| &t.columns) {
        if strip_prefix_ci(&column.name, partial).is_some() && !out.iter().any(|s| s.text == column.name) {
            out.push(Suggestion {
                text: column.name.clone(),
                kind: Kind::Column,
                detail: column.kind.clone().unwrap_or_else(|| "column".into()),
            });
        }
    }
    out
}

/// Tables named after `FROM`/`JOIN`/…, with their aliases (`users u`, `users AS u`).
fn references<'a>(sql: &str, tables: &'a [Table]) -> Vec<(&'a Table, Option<String>)> {
    let items = lex(sql);
    let word = |i: usize| match items.get(i) {
        Some(Item::Word(w)) => Some(w.as_str()),
        _ => None,
    };
    let is_keyword = |w: &str| KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(w));
    let mut refs = Vec::new();
    for i in 0..items.len() {
        let Some(kw) = word(i).filter(|w| TABLE_CONTEXT.iter().any(|k| k.eq_ignore_ascii_case(w))) else {
            continue;
        };
        let mut j = i + 1;
        // `FROM a x, b y` names several tables; the others name one.
        while let Some(name) = word(j) {
            j += 1;
            let mut alias = None;
            if word(j).is_some_and(|w| w.eq_ignore_ascii_case("as")) {
                alias = word(j + 1);
                j += 2;
            } else if let Some(a) = word(j).filter(|w| !is_keyword(w)) {
                alias = Some(a);
                j += 1;
            }
            if let Some(table) = find_table(tables, name) {
                refs.push((table, alias.map(str::to_owned)));
            }
            if !kw.eq_ignore_ascii_case("from") || items.get(j) != Some(&Item::Punct(',')) {
                break;
            }
            j += 1;
        }
    }
    refs
}

/// The table called `name`, or else one whose name without the schema matches.
fn find_table<'a>(tables: &'a [Table], name: &str) -> Option<&'a Table> {
    tables
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(name))
        .or_else(|| tables.iter().find(|t| short(&t.name).eq_ignore_ascii_case(short(name))))
}

/// `users` for `public.users`.
fn short(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// `name` without `prefix`, ignoring ASCII case.
fn strip_prefix_ci<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    let head = name.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| &name[prefix.len()..])
}

#[derive(Debug, PartialEq)]
enum Item {
    /// A name or keyword; `a.b` stays one word, quoted names lose their quotes.
    Word(String),
    Punct(char),
}

/// Words and punctuation of `sql`, skipping comments; literals become `?`.
fn lex(sql: &str) -> Vec<Item> {
    let mut items = Vec::new();
    for (token, range) in tokenize(sql) {
        let text = &sql[range];
        match token {
            Token::Comment => {}
            Token::String if text.starts_with(['"', '`']) => {
                items.push(Item::Word(text.trim_matches(['"', '`']).to_string()));
            }
            Token::String | Token::Parameter => items.push(Item::Punct('?')),
            Token::Plain | Token::Keyword | Token::Function | Token::Constant => {
                let mut word = String::new();
                for c in text.chars() {
                    if c == '.' || c == '_' || c.is_alphanumeric() {
                        word.push(c);
                        continue;
                    }
                    if !word.is_empty() {
                        items.push(Item::Word(std::mem::take(&mut word)));
                    }
                    if !c.is_whitespace() {
                        items.push(Item::Punct(c));
                    }
                }
                if !word.is_empty() {
                    items.push(Item::Word(word));
                }
            }
        }
    }
    items
}

/// Whether byte offset `at` is inside a comment, string or `{{ parameter }}`.
fn in_literal(sql: &str, at: usize) -> bool {
    let Some((kind, range)) = tokenize(sql).into_iter().find(|(_, r)| r.start < at && at <= r.end) else {
        return false;
    };
    let text = &sql[range.clone()];
    let unterminated = match kind {
        // A line comment runs up to (not including) the newline.
        Token::Comment => text.starts_with("--") || text.len() < 4 || !text.ends_with("*/"),
        Token::String => text.len() < 2 || !text.ends_with(&text[..1]),
        Token::Parameter => text.len() < 4 || !text.ends_with("}}"),
        _ => return false,
    };
    at < range.end || unterminated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::TableColumn;

    fn table(name: &str, columns: &[&str]) -> Table {
        let columns =
            columns.iter().map(|c| TableColumn { name: c.to_string(), kind: Some("int".into()) }).collect();
        Table { name: name.into(), columns }
    }

    fn schema() -> Vec<Table> {
        vec![
            table("users", &["id", "email", "plan"]),
            table("orders", &["id", "user_id", "amount"]),
            table("billing.invoices", &["id", "order_id", "paid_at"]),
        ]
    }

    /// Suggestions at the `|` in `sql`.
    fn texts(sql: &str) -> Vec<String> {
        texts_forced(sql, false)
    }

    fn texts_forced(sql: &str, forced: bool) -> Vec<String> {
        let cursor = sql.find('|').unwrap();
        let sql = sql.replace('|', "");
        complete(&sql, cursor, &schema(), forced)
            .map_or_else(Vec::new, |c| c.items.into_iter().map(|s| s.text).collect())
    }

    #[test]
    fn columns_after_alias_or_table_name() {
        assert_eq!(texts("SELECT u.| FROM users u"), ["id", "email", "plan"]);
        assert_eq!(texts("SELECT o.a| FROM users AS u JOIN orders AS o ON"), ["amount"]);
        assert_eq!(texts("SELECT x.| FROM users u, orders x"), ["id", "user_id", "amount"]);
        assert_eq!(texts("SELECT i.p| FROM orders o JOIN billing.invoices i"), ["paid_at"]);
        assert_eq!(texts("SELECT users.e|"), ["email"]);
        assert_eq!(texts("SELECT invoices.o|"), ["order_id"], "table name without its schema");
        assert!(texts("SELECT nope.|").is_empty());
    }

    #[test]
    fn tables_after_from_and_in_a_schema() {
        assert_eq!(texts("SELECT * FROM |"), Vec::<String>::new(), "nothing typed");
        assert_eq!(texts_forced("SELECT * FROM |", true), ["users", "orders", "billing.invoices"]);
        assert_eq!(texts("SELECT * FROM u|"), ["users"]);
        assert_eq!(texts("SELECT * FROM users JOIN inv|"), ["billing.invoices"], "matches without schema");
        assert_eq!(texts("SELECT * FROM billing.|"), ["invoices"]);
    }

    #[test]
    fn mentioned_columns_come_first_then_tables_and_keywords() {
        assert_eq!(
            texts("SELECT u| FROM orders"),
            ["user_id", "users", "unbounded", "union", "unique", "update", "using", "uuid"]
        );
        assert_eq!(
            texts("SELECT o| FROM users"),
            ["orders", "offset", "on", "only", "or", "order", "outer", "over"]
        );
        // No table mentioned yet: columns of every table, after tables and keywords.
        assert_eq!(texts("SELECT pa|"), ["partition", "paid_at"]);
    }

    #[test]
    fn keyword_case_follows_what_was_typed() {
        assert_eq!(texts("sel|"), ["select"]);
        assert_eq!(texts("Sel|"), ["SELECT"]);
    }

    #[test]
    fn nothing_inside_literals_numbers_or_complete_words() {
        assert!(texts("SELECT 'us|'").is_empty());
        assert!(texts("SELECT 'us|").is_empty());
        assert!(texts("-- us|").is_empty());
        assert!(texts("/* us| */").is_empty());
        assert!(texts("SELECT {{ us|").is_empty());
        assert!(texts("SELECT 1.|").is_empty());
        assert!(texts("SELECT |").is_empty());
        assert_eq!(texts("SELECT 'a' use|"), ["users", "user_id"]);
        assert_eq!(texts("/* c */ use|"), ["users", "user_id"]);
        assert!(texts("SELECT * FROM users|").is_empty(), "already typed out");
    }

    #[test]
    fn replaces_the_partial_word() {
        let c = complete("SELECT ё, u.em FROM users u", 14, &schema(), false).unwrap();
        assert_eq!(c.replace, 12..14);
        assert_eq!(c.items[0].detail, "int");
        let (sql, cursor) = apply("SELECT ё, u.em FROM users u", &c.replace, "email");
        assert_eq!((sql.as_str(), cursor), ("SELECT ё, u.email FROM users u", 17));
    }
}
