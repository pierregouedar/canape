//! Valve KeyValues text format (`.vdf`, `.acf`): nested `"key" "value"` and
//! `"key" { … }` pairs. Tolerant: comments, unquoted tokens and `[$WIN32]`
//! conditions are accepted, and a truncated file yields what was read.

/// A value: a string or a list of children, in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Str(String),
    Obj(Vec<(String, Value)>),
}

impl Value {
    /// First child named `key`, ignoring case (Steam is not consistent:
    /// `apps` and `Apps`, `LastPlayed` and `lastplayed`).
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries()
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    /// Follows a path of keys.
    pub fn path(&self, keys: &[&str]) -> Option<&Value> {
        keys.iter().try_fold(self, |value, key| value.get(key))
    }

    /// String value of the child `key`.
    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Str(s) => Some(s),
            Value::Obj(_) => None,
        }
    }

    /// Children of an object; empty for a string.
    pub fn entries(&self) -> &[(String, Value)] {
        match self {
            Value::Obj(entries) => entries,
            Value::Str(_) => &[],
        }
    }
}

#[derive(Debug, PartialEq)]
enum Token {
    Text(String),
    Open,
    Close,
}

struct Lexer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl Iterator for Lexer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        loop {
            let c = *self.chars.peek()?;
            if c.is_whitespace() {
                self.chars.next();
            } else if c == '/' {
                // `//` comment to the end of the line; a lone `/` is text.
                self.chars.next();
                if self.chars.peek() == Some(&'/') {
                    for c in self.chars.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                } else {
                    return Some(Token::Text(format!("/{}", self.bare())));
                }
            } else if c == '[' {
                // Platform condition such as `[$WIN32]`: ignored.
                for c in self.chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                }
            } else if c == '{' {
                self.chars.next();
                return Some(Token::Open);
            } else if c == '}' {
                self.chars.next();
                return Some(Token::Close);
            } else if c == '"' {
                self.chars.next();
                return Some(Token::Text(self.quoted()));
            } else {
                return Some(Token::Text(self.bare()));
            }
        }
    }
}

impl Lexer<'_> {
    fn quoted(&mut self) -> String {
        let mut out = String::new();
        while let Some(c) = self.chars.next() {
            match c {
                '"' => break,
                '\\' => match self.chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => break,
                },
                c => out.push(c),
            }
        }
        out
    }

    fn bare(&mut self) -> String {
        let mut out = String::new();
        while let Some(&c) = self.chars.peek() {
            if c.is_whitespace() || matches!(c, '{' | '}' | '"') {
                break;
            }
            out.push(c);
            self.chars.next();
        }
        out
    }
}

/// Parses a whole file into its top-level object.
pub fn parse(text: &str) -> Value {
    let mut lexer = Lexer {
        chars: text.trim_start_matches('\u{feff}').chars().peekable(),
    };
    Value::Obj(object(&mut lexer))
}

fn object(lexer: &mut Lexer) -> Vec<(String, Value)> {
    let mut entries = Vec::new();
    while let Some(token) = lexer.next() {
        let key = match token {
            Token::Text(key) => key,
            Token::Close => break,
            // A stray `{`: read and drop the block.
            Token::Open => {
                object(lexer);
                continue;
            }
        };
        match lexer.next() {
            Some(Token::Text(value)) => entries.push((key, Value::Str(value))),
            Some(Token::Open) => {
                let children = object(lexer);
                entries.push((key, Value::Obj(children)));
            }
            Some(Token::Close) | None => break,
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_objects_and_escapes() {
        let v = parse(
            r#"
            // comment
            "libraryfolders"
            {
                "0"
                {
                    "path"		"C:\\Program Files (x86)\\Steam"
                    "apps" { "228980" "123" "1245620" "456" }
                }
                "1" { "path" "D:\\Games \"Steam\"" }
            }"#,
        );
        let folders = v.get("libraryfolders").unwrap();
        assert_eq!(
            folders.path(&["0"]).unwrap().str("PATH"),
            Some(r"C:\Program Files (x86)\Steam")
        );
        assert_eq!(
            folders.path(&["0", "apps"]).unwrap().entries().len(),
            2,
            "apps"
        );
        assert_eq!(
            folders.path(&["1"]).unwrap().str("path"),
            Some(r#"D:\Games "Steam""#)
        );
    }

    #[test]
    fn tolerates_conditions_bare_tokens_and_truncation() {
        let v = parse("root { key value [$WIN32] other \"x\" broken { \"a\" \"b\"");
        let root = v.get("root").unwrap();
        assert_eq!(root.str("key"), Some("value"));
        assert_eq!(root.str("other"), Some("x"));
        assert_eq!(root.path(&["broken"]).unwrap().str("a"), Some("b"));
        assert_eq!(parse(""), Value::Obj(Vec::new()));
        assert_eq!(parse("}}}"), Value::Obj(Vec::new()));
    }
}
