//! EDS file syntax: `[Section]` headers, `Keyword = field, field, ...;`
//! entries, `$` comments to end of line, and double-quoted strings. Adjacent
//! strings in one field are joined, which is how long strings span lines.

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    Empty,
    /// A quoted string, without the quotes.
    Str(String),
    /// Unquoted text, trimmed: a number, a reference like `Param4`, etc.
    Token(String),
}

impl Field {
    /// The text of a string or token; `None` for an empty field.
    pub fn text(&self) -> Option<&str> {
        match self {
            Field::Empty => None,
            Field::Str(s) | Field::Token(s) => Some(s),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub keyword: String,
    pub fields: Vec<Field>,
    /// Line of the keyword, 1-based.
    pub line: usize,
}

impl Entry {
    pub fn field(&self, i: usize) -> &Field {
        self.fields.get(i).unwrap_or(&Field::Empty)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub entries: Vec<Entry>,
}

impl Section {
    /// Finds an entry by keyword, ignoring case.
    pub fn get(&self, keyword: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.keyword.eq_ignore_ascii_case(keyword))
    }
}

struct Cursor<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    line: usize,
}

impl Cursor<'_> {
    fn next(&mut self) -> Option<char> {
        let c = self.chars.next();
        if c == Some('\n') {
            self.line += 1;
        }
        c
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn skip_comment(&mut self) {
        while let Some(c) = self.next() {
            if c == '\n' {
                break;
            }
        }
    }

    fn skip_blank(&mut self) {
        while let Some(c) = self.peek() {
            if c == '$' {
                self.skip_comment();
            } else if c.is_whitespace() {
                self.next();
            } else {
                break;
            }
        }
    }

    fn error(&self, msg: impl std::fmt::Display) -> Error {
        Error::Eds(format!("line {}: {msg}", self.line))
    }
}

pub fn parse(text: &str) -> Result<Vec<Section>> {
    let mut cur = Cursor {
        chars: text.chars().peekable(),
        line: 1,
    };
    let mut sections: Vec<Section> = Vec::new();
    loop {
        cur.skip_blank();
        match cur.peek() {
            None => break,
            Some('[') => {
                cur.next();
                let mut name = String::new();
                loop {
                    match cur.next() {
                        Some(']') => break,
                        Some('\n') | None => return Err(cur.error("unterminated section header")),
                        Some(c) => name.push(c),
                    }
                }
                sections.push(Section {
                    name: name.trim().to_string(),
                    entries: Vec::new(),
                });
            }
            Some(_) => {
                let line = cur.line;
                let entry = parse_entry(&mut cur, line)?;
                let section = sections.last_mut().ok_or_else(|| {
                    cur.error(format!("entry '{}' before any section", entry.keyword))
                })?;
                section.entries.push(entry);
            }
        }
    }
    Ok(sections)
}

fn parse_entry(cur: &mut Cursor<'_>, line: usize) -> Result<Entry> {
    let mut keyword = String::new();
    loop {
        match cur.next() {
            Some('=') => break,
            Some(c @ (';' | '[' | '"' | ',')) => {
                return Err(cur.error(format!(
                    "expected '=' after '{}', found '{c}'",
                    keyword.trim()
                )));
            }
            None => return Err(cur.error(format!("expected '=' after '{}'", keyword.trim()))),
            Some(c) => keyword.push(c),
        }
    }
    let keyword = keyword.trim().to_string();

    let mut fields = Vec::new();
    let mut strings: Option<String> = None;
    let mut token = String::new();
    loop {
        cur.skip_blank();
        match cur.next() {
            Some('"') => {
                let s = strings.get_or_insert_with(String::new);
                loop {
                    match cur.next() {
                        Some('"') => break,
                        Some('\\') => match cur.next() {
                            Some('n') => s.push('\n'),
                            Some(c) => s.push(c),
                            None => return Err(cur.error("unterminated string")),
                        },
                        Some(c) => s.push(c),
                        None => return Err(cur.error("unterminated string")),
                    }
                }
            }
            Some(c @ (',' | ';')) => {
                fields.push(match strings.take() {
                    Some(s) => Field::Str(s),
                    None if token.trim().is_empty() => Field::Empty,
                    None => Field::Token(token.trim().to_string()),
                });
                token.clear();
                if c == ';' {
                    return Ok(Entry {
                        keyword,
                        fields,
                        line,
                    });
                }
            }
            Some(c) => {
                token.push(c);
                // Keep the rest of the token together, stopping at anything
                // skip_blank or the match above must see.
                while let Some(c) = cur.peek() {
                    if matches!(c, ',' | ';' | '"' | '$' | '\n') {
                        break;
                    }
                    token.push(c);
                    cur.next();
                }
            }
            None => {
                return Err(Error::Eds(format!(
                    "line {line}: entry '{keyword}' has no terminating ';'"
                )));
            }
        }
    }
}

/// Parses a decimal or `0x` hexadecimal integer.
pub fn parse_int(s: &str) -> Option<u64> {
    let s = s.trim();
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_entries_and_fields() {
        let sections = parse(
            "$ header comment\n\
             [File]\n\
             \tDescText = \"EDS for a test\";   $ trailing comment\n\
             \tRevision = 2.3;\n\
             [Connection Manager]\n\
             \tConnection1 =\n\
             \t\t0x84010002,   $ comment, with a comma; and a semicolon\n\
             \t\tParam4,,Assem150,\n\
             \t\t\"Exclusive \" \"Owner\",\n\
             \t\t\"20 04 24 97\";\n",
        )
        .unwrap();
        assert_eq!(sections.len(), 2);
        assert_eq!(
            sections[0].get("desctext").unwrap().fields,
            [Field::Str("EDS for a test".into())]
        );
        assert_eq!(
            sections[0].get("Revision").unwrap().fields,
            [Field::Token("2.3".into())]
        );

        let conn = sections[1].get("Connection1").unwrap();
        assert_eq!(conn.line, 6);
        assert_eq!(
            conn.fields,
            [
                Field::Token("0x84010002".into()),
                Field::Token("Param4".into()),
                Field::Empty,
                Field::Token("Assem150".into()),
                Field::Str("Exclusive Owner".into()),
                Field::Str("20 04 24 97".into()),
            ]
        );
    }

    #[test]
    fn errors_have_line_numbers() {
        let err = parse("[A]\n  Key = 1,\n  2\n").unwrap_err().to_string();
        assert!(
            err.contains("line 2") && err.contains("no terminating"),
            "{err}"
        );
        let err = parse("Key = 1;").unwrap_err().to_string();
        assert!(err.contains("before any section"), "{err}");
        let err = parse("[A]\n Key = \"open\n").unwrap_err().to_string();
        assert!(err.contains("unterminated string"), "{err}");
    }

    #[test]
    fn integers() {
        assert_eq!(parse_int("0x44640405"), Some(0x44640405));
        assert_eq!(parse_int(" 20000 "), Some(20000));
        assert_eq!(parse_int("Param4"), None);
    }
}
