//! The CSS tokenizer: text in, a flat list of tokens with positions out.
//!
//! Positions are 1-based and in characters. The caller gives the position of
//! the first character, so CSS inside a `<style>` block reports places in the
//! HTML file.

use crate::error::MarkupError;

/// What a token is.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub enum Tok {
    /// A name: `red`, `flex-start`, `--gap`.
    Ident(String),
    /// `@keyframes`, without the `@`.
    AtKeyword(String),
    /// `#id` or `#ff8800`, without the `#`.
    Hash(String),
    /// A quoted string, escapes already decoded.
    Str(String),
    /// `url(name)` written without quotes.
    Url(String),
    /// A plain number.
    Number(f32),
    /// `50%`: the number is 50.
    Percent(f32),
    /// `12px`: a number and its unit as written.
    Dimension {
        value: f32,
        unit: String,
    },
    /// A name directly followed by `(`; the `(` is part of the token.
    Function(String),
    /// Any other single character: `>`, `+`, `.`, `!`, `/`, `*`.
    Delim(char),
    Colon,
    Semi,
    Comma,
    LBrace,
    RBrace,
    LParen,
    RParen,
    /// A run of spaces, tabs and newlines.
    Whitespace,
}

/// A token and where it starts.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    /// What it is.
    pub tok: Tok,
    /// 1-based line.
    pub line: u32,
    /// 1-based column.
    pub column: u32,
}

impl Token {
    /// The token as a short piece of text for an error message.
    pub fn describe(&self) -> String {
        match &self.tok {
            Tok::Ident(s) => s.clone(),
            Tok::AtKeyword(s) => format!("@{s}"),
            Tok::Hash(s) => format!("#{s}"),
            Tok::Str(s) => format!("\"{s}\""),
            Tok::Url(s) => format!("url({s})"),
            Tok::Number(v) => fmt_num(*v),
            Tok::Percent(v) => format!("{}%", fmt_num(*v)),
            Tok::Dimension { value, unit } => format!("{}{unit}", fmt_num(*value)),
            Tok::Function(s) => format!("{s}("),
            Tok::Delim(c) => c.to_string(),
            Tok::Colon => ":".into(),
            Tok::Semi => ";".into(),
            Tok::Comma => ",".into(),
            Tok::LBrace => "{".into(),
            Tok::RBrace => "}".into(),
            Tok::LParen => "(".into(),
            Tok::RParen => ")".into(),
            Tok::Whitespace => "whitespace".into(),
        }
    }
}

/// A number without a trailing `.0`.
pub(crate) fn fmt_num(v: f32) -> String {
    if v.fract() == 0.0 && v.abs() < 1e9 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

struct Lexer<'a> {
    file: &'a str,
    chars: Vec<char>,
    i: usize,
    line: u32,
    col: u32,
}

/// Splits `text` into tokens. `line` and `column` are where `text` starts.
pub fn tokenize(file: &str, text: &str, line: u32, column: u32) -> Result<Vec<Token>, MarkupError> {
    let mut lx = Lexer {
        file,
        chars: text.chars().collect(),
        i: 0,
        line,
        col: column,
    };
    let mut out = Vec::new();
    while lx.i < lx.chars.len() {
        if let Some(t) = lx.next_token()? {
            out.push(t);
        }
    }
    Ok(out)
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || !c.is_ascii()
}

fn is_name_char(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-'
}

impl Lexer<'_> {
    fn peek(&self, off: usize) -> Option<char> {
        self.chars.get(self.i + off).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek(0)?;
        self.i += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn err(&self, line: u32, col: u32, msg: &str, help: &str) -> MarkupError {
        MarkupError::new(self.file, line, col, msg, help)
    }

    fn starts_number(&self) -> bool {
        let digit = |o: usize| self.peek(o).is_some_and(|c| c.is_ascii_digit());
        match self.peek(0) {
            Some(c) if c.is_ascii_digit() => true,
            Some('.') => digit(1),
            Some('+' | '-') => digit(1) || (self.peek(1) == Some('.') && digit(2)),
            _ => false,
        }
    }

    fn starts_ident_at(&self, off: usize) -> bool {
        match self.peek(off) {
            Some(c) if is_name_start(c) || c == '\\' => true,
            Some('-') => self
                .peek(off + 1)
                .is_some_and(|c| is_name_start(c) || c == '-' || c == '\\'),
            _ => false,
        }
    }

    fn escape(&mut self) -> char {
        // The backslash is already consumed.
        match self.peek(0) {
            None => '\u{fffd}',
            Some(c) if c.is_ascii_hexdigit() => {
                let mut code = 0u32;
                let mut n = 0;
                while n < 6 {
                    match self.peek(0).and_then(|c| c.to_digit(16)) {
                        Some(d) => {
                            code = code * 16 + d;
                            self.bump();
                            n += 1;
                        }
                        None => break,
                    }
                }
                if self.peek(0).is_some_and(char::is_whitespace) {
                    self.bump();
                }
                char::from_u32(code).unwrap_or('\u{fffd}')
            }
            Some(c) => {
                self.bump();
                c
            }
        }
    }

    fn name(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek(0) {
            if is_name_char(c) {
                s.push(c);
                self.bump();
            } else if c == '\\' && self.peek(1).is_some() {
                self.bump();
                s.push(self.escape());
            } else {
                break;
            }
        }
        s
    }

    fn number(&mut self) -> Result<f32, MarkupError> {
        let (line, col) = (self.line, self.col);
        let mut s = String::new();
        if let Some(c @ ('+' | '-')) = self.peek(0) {
            s.push(c);
            self.bump();
        }
        while let Some(c) = self.peek(0).filter(char::is_ascii_digit) {
            s.push(c);
            self.bump();
        }
        if self.peek(0) == Some('.') && self.peek(1).is_some_and(|c| c.is_ascii_digit()) {
            s.push('.');
            self.bump();
            while let Some(c) = self.peek(0).filter(char::is_ascii_digit) {
                s.push(c);
                self.bump();
            }
        }
        if matches!(self.peek(0), Some('e' | 'E')) {
            let digit_at = |o: usize| self.peek(o).is_some_and(|c| c.is_ascii_digit());
            let signed = matches!(self.peek(1), Some('+' | '-')) && digit_at(2);
            if digit_at(1) || signed {
                s.push('e');
                self.bump();
                if signed && let Some(c) = self.bump() {
                    s.push(c);
                }
                while let Some(c) = self.peek(0).filter(char::is_ascii_digit) {
                    s.push(c);
                    self.bump();
                }
            }
        }
        match s.parse::<f32>() {
            Ok(v) if v.is_finite() => Ok(v),
            _ => Err(self.err(
                line,
                col,
                &format!("`{s}` is not a usable number"),
                "write a number like 12 or 0.5",
            )),
        }
    }

    fn string(&mut self, quote: char) -> Result<String, MarkupError> {
        let (line, col) = (self.line, self.col);
        self.bump();
        let mut s = String::new();
        loop {
            match self.peek(0) {
                None | Some('\n') => {
                    return Err(self.err(
                        line,
                        col,
                        "this string is never closed",
                        &format!("end it with a matching {quote} on the same line"),
                    ));
                }
                Some(c) if c == quote => {
                    self.bump();
                    return Ok(s);
                }
                Some('\\') => {
                    self.bump();
                    if self.peek(0) == Some('\n') {
                        self.bump();
                    } else {
                        s.push(self.escape());
                    }
                }
                Some(c) => {
                    s.push(c);
                    self.bump();
                }
            }
        }
    }

    fn next_token(&mut self) -> Result<Option<Token>, MarkupError> {
        let (line, column) = (self.line, self.col);
        let Some(c) = self.peek(0) else {
            return Ok(None);
        };
        let tok = if c.is_whitespace() {
            while self.peek(0).is_some_and(char::is_whitespace) {
                self.bump();
            }
            Tok::Whitespace
        } else if c == '/' && self.peek(1) == Some('*') {
            self.bump();
            self.bump();
            loop {
                match self.peek(0) {
                    None => {
                        return Err(self.err(
                            line,
                            column,
                            "this comment is never closed",
                            "end it with */",
                        ));
                    }
                    Some('*') if self.peek(1) == Some('/') => {
                        self.bump();
                        self.bump();
                        break;
                    }
                    Some(_) => {
                        self.bump();
                    }
                }
            }
            return Ok(None);
        } else if c == '"' || c == '\'' {
            Tok::Str(self.string(c)?)
        } else if self.starts_number() {
            let value = self.number()?;
            if self.peek(0) == Some('%') {
                self.bump();
                Tok::Percent(value)
            } else if self.starts_ident_at(0) {
                Tok::Dimension {
                    value,
                    unit: self.name(),
                }
            } else {
                Tok::Number(value)
            }
        } else if self.starts_ident_at(0) {
            let name = self.name();
            if self.peek(0) == Some('(') {
                self.bump();
                if name.eq_ignore_ascii_case("url") {
                    self.url_or_function(name, line, column)?
                } else {
                    Tok::Function(name)
                }
            } else {
                Tok::Ident(name)
            }
        } else if c == '#' && self.peek(1).is_some_and(is_name_char) {
            self.bump();
            Tok::Hash(self.name())
        } else if c == '@' && self.starts_ident_at(1) {
            self.bump();
            Tok::AtKeyword(self.name())
        } else {
            self.bump();
            match c {
                ':' => Tok::Colon,
                ';' => Tok::Semi,
                ',' => Tok::Comma,
                '{' => Tok::LBrace,
                '}' => Tok::RBrace,
                '(' => Tok::LParen,
                ')' => Tok::RParen,
                other => Tok::Delim(other),
            }
        };
        Ok(Some(Token { tok, line, column }))
    }

    /// After `url(`: a quoted name stays an ordinary function, a bare one is
    /// read up to the `)`.
    fn url_or_function(&mut self, name: String, line: u32, col: u32) -> Result<Tok, MarkupError> {
        let mut ahead = 0;
        while self.peek(ahead).is_some_and(char::is_whitespace) {
            ahead += 1;
        }
        if matches!(self.peek(ahead), Some('"' | '\'')) {
            return Ok(Tok::Function(name));
        }
        while self.peek(0).is_some_and(char::is_whitespace) {
            self.bump();
        }
        let mut s = String::new();
        loop {
            match self.peek(0) {
                None => {
                    return Err(self.err(
                        line,
                        col,
                        "this url( is never closed",
                        "end it with a )",
                    ));
                }
                Some(')') => {
                    self.bump();
                    break;
                }
                Some('\\') => {
                    self.bump();
                    s.push(self.escape());
                }
                Some(c) => {
                    s.push(c);
                    self.bump();
                }
            }
        }
        Ok(Tok::Url(s.trim().to_owned()))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        tokenize("t.css", s, 1, 1)
            .unwrap()
            .into_iter()
            .map(|t| t.tok)
            .filter(|t| *t != Tok::Whitespace)
            .collect()
    }

    #[test]
    fn numbers() {
        assert_eq!(toks(".5"), vec![Tok::Number(0.5)]);
        assert_eq!(toks("-1e2"), vec![Tok::Number(-100.0)]);
        assert_eq!(toks("+3"), vec![Tok::Number(3.0)]);
        assert_eq!(toks("10%"), vec![Tok::Percent(10.0)]);
        assert_eq!(toks("1.5e-1"), vec![Tok::Number(0.15)]);
        assert_eq!(
            toks("2em"),
            vec![Tok::Dimension {
                value: 2.0,
                unit: "em".into()
            }]
        );
        assert_eq!(
            toks("1e3px"),
            vec![Tok::Dimension {
                value: 1000.0,
                unit: "px".into()
            }]
        );
    }

    #[test]
    fn idents_and_hashes() {
        assert_eq!(
            toks("--gap flex-start #ff8800 @keyframes"),
            vec![
                Tok::Ident("--gap".into()),
                Tok::Ident("flex-start".into()),
                Tok::Hash("ff8800".into()),
                Tok::AtKeyword("keyframes".into()),
            ]
        );
        assert_eq!(toks("-"), vec![Tok::Delim('-')]);
        assert_eq!(
            toks("a-b"),
            vec![Tok::Ident("a-b".into())],
            "dashes stay inside names"
        );
    }

    #[test]
    fn comments_vanish() {
        assert_eq!(
            toks("a/* x { } */b"),
            vec![Tok::Ident("a".into()), Tok::Ident("b".into())]
        );
        assert!(tokenize("t.css", "a /* never", 1, 1).is_err());
    }

    #[test]
    fn strings_with_escapes() {
        assert_eq!(toks(r#""a\"b""#), vec![Tok::Str("a\"b".into())]);
        assert_eq!(toks(r"'A\42 C'"), vec![Tok::Str("ABC".into())]);
        assert_eq!(toks("'a\\\nb'"), vec![Tok::Str("ab".into())]);
        let e = tokenize("t.css", "x: \"open\ny", 1, 1).unwrap_err();
        assert_eq!((e.line, e.column), (1, 4));
    }

    #[test]
    fn functions_and_urls() {
        assert_eq!(
            toks("calc(1px)"),
            vec![
                Tok::Function("calc".into()),
                Tok::Dimension {
                    value: 1.0,
                    unit: "px".into()
                },
                Tok::RParen
            ]
        );
        assert_eq!(toks("url( a/b.png )"), vec![Tok::Url("a/b.png".into())]);
        assert_eq!(
            toks("url('a.png')"),
            vec![
                Tok::Function("url".into()),
                Tok::Str("a.png".into()),
                Tok::RParen
            ]
        );
        assert!(tokenize("t.css", "url(abc", 1, 1).is_err());
    }

    #[test]
    fn positions_follow_lines_and_the_start_offset() {
        let t = tokenize("p.html", "a\n  b", 10, 5).unwrap();
        let pos: Vec<(u32, u32)> = t.iter().map(|t| (t.line, t.column)).collect();
        assert_eq!(pos, vec![(10, 5), (10, 6), (11, 3)]);
    }

    #[test]
    fn delims() {
        assert_eq!(
            toks("a > b!"),
            vec![
                Tok::Ident("a".into()),
                Tok::Delim('>'),
                Tok::Ident("b".into()),
                Tok::Delim('!')
            ]
        );
    }
}
