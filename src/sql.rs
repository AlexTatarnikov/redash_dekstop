//! SQL tokenizer for syntax highlighting. Pure: the editor maps each [`Token`] kind
//! to a colour in `ui/theme.rs`.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    /// Identifiers, operators, punctuation, whitespace.
    Plain,
    /// Reserved words and type names (`SELECT`, `JOIN`, `INT`).
    Keyword,
    /// A name followed by `(`, e.g. `count(`.
    Function,
    /// Numbers and `NULL`, `TRUE`, `FALSE`.
    Constant,
    /// `'…'` literals and `"…"` / `` `…` `` quoted identifiers.
    String,
    /// `-- …` and `/* … */`.
    Comment,
    /// Redash query parameters: `{{ name }}`.
    Parameter,
}

pub(crate) const KEYWORDS: &[&str] = &[
    "all",
    "alter",
    "and",
    "any",
    "array",
    "as",
    "asc",
    "begin",
    "between",
    "bigint",
    "boolean",
    "both",
    "by",
    "case",
    "cast",
    "char",
    "check",
    "column",
    "commit",
    "constraint",
    "create",
    "cross",
    "cube",
    "date",
    "decimal",
    "default",
    "delete",
    "desc",
    "distinct",
    "double",
    "drop",
    "else",
    "end",
    "except",
    "exists",
    "explain",
    "fetch",
    "filter",
    "first",
    "float",
    "following",
    "for",
    "foreign",
    "from",
    "full",
    "grant",
    "group",
    "having",
    "if",
    "ilike",
    "in",
    "index",
    "inner",
    "insert",
    "int",
    "integer",
    "intersect",
    "interval",
    "into",
    "is",
    "join",
    "json",
    "jsonb",
    "key",
    "last",
    "lateral",
    "left",
    "like",
    "limit",
    "natural",
    "not",
    "nulls",
    "numeric",
    "offset",
    "on",
    "only",
    "or",
    "order",
    "outer",
    "over",
    "partition",
    "preceding",
    "primary",
    "range",
    "real",
    "recursive",
    "references",
    "returning",
    "right",
    "rollback",
    "rollup",
    "row",
    "rows",
    "select",
    "set",
    "smallint",
    "table",
    "text",
    "then",
    "time",
    "timestamp",
    "timestamptz",
    "to",
    "truncate",
    "unbounded",
    "union",
    "unique",
    "update",
    "using",
    "uuid",
    "values",
    "varchar",
    "view",
    "when",
    "where",
    "window",
    "with",
    "within",
];

const CONSTANTS: &[&str] = &["null", "true", "false"];

/// Splits `sql` into highlighted spans (byte ranges) that cover the whole text.
pub fn tokenize(sql: &str) -> Vec<(Token, Range<usize>)> {
    let bytes = sql.as_bytes();
    let mut out: Vec<(Token, Range<usize>)> = Vec::new();
    let mut push = |kind: Token, range: Range<usize>| match out.last_mut() {
        Some((last, r)) if *last == kind && r.end == range.start => r.end = range.end,
        _ => out.push((kind, range)),
    };
    // Byte offset of the first occurrence of `pat` at or after `from`, or the end.
    let find = |from: usize, pat: &str| sql[from..].find(pat).map_or(sql.len(), |i| from + i);

    let mut i = 0;
    while i < bytes.len() {
        let rest = &sql[i..];
        let (kind, end) = if rest.starts_with("--") {
            (Token::Comment, find(i, "\n"))
        } else if rest.starts_with("/*") {
            (Token::Comment, (find(i + 2, "*/") + 2).min(sql.len()))
        } else if rest.starts_with("{{") {
            (Token::Parameter, (find(i + 2, "}}") + 2).min(sql.len()))
        } else if let Some(q @ (b'\'' | b'"' | b'`')) = bytes.get(i).copied() {
            (Token::String, quoted_end(bytes, i, q))
        } else if bytes[i].is_ascii_digit() {
            let end = scan(bytes, i, |b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_');
            (Token::Constant, end)
        } else if is_word(bytes[i]) {
            let end = scan(bytes, i, is_word);
            let word = sql[i..end].to_ascii_lowercase();
            let kind = if CONSTANTS.contains(&word.as_str()) {
                Token::Constant
            } else if sql[end..].trim_start_matches([' ', '\t']).starts_with('(')
                && !KEYWORDS.contains(&word.as_str())
            {
                Token::Function
            } else if KEYWORDS.contains(&word.as_str()) && !preceded_by_dot(bytes, i) {
                Token::Keyword
            } else {
                Token::Plain
            };
            (kind, end)
        } else {
            let len = rest.chars().next().map_or(1, char::len_utf8);
            (Token::Plain, i + len)
        };
        push(kind, i..end);
        i = end;
    }
    out
}

pub(crate) fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

fn scan(bytes: &[u8], start: usize, pred: impl Fn(u8) -> bool) -> usize {
    bytes[start..].iter().position(|&b| !pred(b)).map_or(bytes.len(), |n| start + n)
}

/// End of the literal opened by `quote` at `start`; a doubled quote is an escape.
fn quoted_end(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == quote {
            if bytes.get(i + 1) == Some(&quote) {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

/// `t.date` is a column, not the `DATE` keyword.
fn preceded_by_dot(bytes: &[u8], i: usize) -> bool {
    i > 0 && bytes[i - 1] == b'.'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(sql: &str) -> Vec<(Token, &str)> {
        tokenize(sql).into_iter().map(|(k, r)| (k, sql[r].trim())).filter(|(_, s)| !s.is_empty()).collect()
    }

    #[test]
    fn highlights_keywords_functions_and_literals() {
        use Token::*;
        assert_eq!(
            spans("SELECT count(*), 'it''s', 4.5, NULL FROM t.date WHERE x IS not null"),
            [
                (Keyword, "SELECT"),
                (Function, "count"),
                (Plain, "(*),"),
                (String, "'it''s'"),
                (Plain, ","),
                (Constant, "4.5"),
                (Plain, ","),
                (Constant, "NULL"),
                (Keyword, "FROM"),
                (Plain, "t.date"),
                (Keyword, "WHERE"),
                (Plain, "x"),
                (Keyword, "IS"),
                (Keyword, "not"),
                (Constant, "null"),
            ]
        );
    }

    #[test]
    fn highlights_comments_quoted_names_and_parameters() {
        use Token::*;
        assert_eq!(
            spans("select \"Name\" -- who\n/* multi\nline */ where d > '{{ start }}' and p = {{ plan }}"),
            [
                (Keyword, "select"),
                (String, "\"Name\""),
                (Comment, "-- who"),
                (Comment, "/* multi\nline */"),
                (Keyword, "where"),
                (Plain, "d >"),
                (String, "'{{ start }}'"),
                (Keyword, "and"),
                (Plain, "p ="),
                (Parameter, "{{ plan }}"),
            ]
        );
    }

    #[test]
    fn spans_cover_text_including_unterminated_and_unicode() {
        for sql in ["select 'abc", "/* open", "{{ p", "SELECT ё, \"x", "where a = b"] {
            let tokens = tokenize(sql);
            let mut pos = 0;
            for (_, r) in &tokens {
                assert_eq!(r.start, pos, "{sql}");
                assert!(sql.is_char_boundary(r.end), "{sql}");
                pos = r.end;
            }
            assert_eq!(pos, sql.len(), "{sql}");
        }
    }
}
