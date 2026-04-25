use miette::SourceSpan;
use std::fmt;

/// Byte-offset span in source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Span {
    pub offset: usize,
    pub len: usize,
}

impl Span {
    pub fn new(offset: usize, len: usize) -> Self {
        Self { offset, len }
    }

    pub fn empty(offset: usize) -> Self {
        Self { offset, len: 0 }
    }

    /// Merge two spans into one covering both.
    pub fn merge(self, other: Span) -> Span {
        let start = self.offset.min(other.offset);
        let end = (self.offset + self.len).max(other.offset + other.len);
        Span::new(start, end - start)
    }
}

impl From<Span> for SourceSpan {
    fn from(s: Span) -> Self {
        SourceSpan::new(s.offset.into(), s.len)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    /// Unquoted identifier: keywords, job keys, day names, numbers, durations, etc.
    /// May contain: a-z A-Z 0-9 : / * ? - . _
    Ident(String),

    /// Quoted string: "..."
    QuotedString(String),

    /// Comment: # ... until end of line (content excludes the #)
    Comment(String),

    /// Placeholder: {env.VAR}, {$VAR}, {$VAR:default}, {vars.NAME}, {file./path}
    Placeholder(String),

    /// `{`
    LBrace,

    /// `}`
    RBrace,

    /// `;`
    Semicolon,

    /// `..` range operator (for windows like 02:00..06:00)
    DotDot,

    /// Newline — acts as statement terminator (like semicolon)
    Newline,

    /// End of input
    Eof,
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenKind::Ident(s) => write!(f, "{s}"),
            TokenKind::QuotedString(s) => write!(f, "\"{s}\""),
            TokenKind::Comment(s) => write!(f, "# {s}"),
            TokenKind::Placeholder(s) => write!(f, "{{{s}}}"),
            TokenKind::LBrace => write!(f, "{{"),
            TokenKind::RBrace => write!(f, "}}"),
            TokenKind::Semicolon => write!(f, ";"),
            TokenKind::DotDot => write!(f, ".."),
            TokenKind::Newline => write!(f, "<newline>"),
            TokenKind::Eof => write!(f, "<eof>"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }

    /// Returns the text content for ident/string tokens.
    pub fn text(&self) -> &str {
        match &self.kind {
            TokenKind::Ident(s) | TokenKind::QuotedString(s) | TokenKind::Placeholder(s) => s,
            _ => "",
        }
    }

    pub fn is_ident(&self, name: &str) -> bool {
        matches!(&self.kind, TokenKind::Ident(s) if s == name)
    }
}

/// Lexer error.
#[derive(Debug, Clone, thiserror::Error, miette::Diagnostic)]
pub enum LexError {
    #[error("unterminated string")]
    #[diagnostic(code(croniq::lex::unterminated_string))]
    UnterminatedString {
        #[label("string starts here")]
        span: SourceSpan,
    },

    #[error("unterminated placeholder")]
    #[diagnostic(code(croniq::lex::unterminated_placeholder))]
    UnterminatedPlaceholder {
        #[label("placeholder starts here")]
        span: SourceSpan,
    },

    #[error("invalid escape sequence '\\{ch}'")]
    #[diagnostic(code(croniq::lex::invalid_escape))]
    InvalidEscape {
        ch: char,
        #[label("here")]
        span: SourceSpan,
    },
}

/// Tokenizes a Croniqfile source string.
pub struct Lexer<'src> {
    source: &'src str,
    bytes: &'src [u8],
    pos: usize,
}

impl<'src> Lexer<'src> {
    pub fn new(source: &'src str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
        }
    }

    /// Tokenize the entire source into a vec of tokens.
    pub fn tokenize(source: &str) -> Result<Vec<Token>, LexError> {
        let mut lexer = Lexer::new(source);
        let mut tokens = Vec::new();
        loop {
            let tok = lexer.next_token()?;
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.bytes.get(self.pos).copied()?;
        self.pos += 1;
        Some(b)
    }

    fn skip_spaces(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// Returns the next token.
    pub fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_spaces();

        let start = self.pos;

        let Some(b) = self.peek() else {
            return Ok(Token::new(TokenKind::Eof, Span::empty(start)));
        };

        // Emit newline token
        if b == b'\n' {
            self.pos += 1;
            // Consume consecutive newlines as one
            while self.peek() == Some(b'\n')
                || self.peek() == Some(b'\r')
                || self.peek() == Some(b' ')
                || self.peek() == Some(b'\t')
            {
                self.pos += 1;
            }
            return Ok(Token::new(TokenKind::Newline, Span::new(start, 1)));
        }

        match b {
            b'#' => self.lex_comment(start),
            b'"' => self.lex_string(start),
            b'{' => {
                // Could be LBrace or Placeholder
                // Placeholder: {$..}, {env.}, {file.}, {vars.}
                if self.is_placeholder_start() {
                    self.lex_placeholder(start)
                } else {
                    self.pos += 1;
                    Ok(Token::new(TokenKind::LBrace, Span::new(start, 1)))
                }
            }
            b'}' => {
                self.pos += 1;
                Ok(Token::new(TokenKind::RBrace, Span::new(start, 1)))
            }
            b';' => {
                self.pos += 1;
                Ok(Token::new(TokenKind::Semicolon, Span::new(start, 1)))
            }
            b'.' if self.peek_at(1) == Some(b'.') => {
                self.pos += 2;
                Ok(Token::new(TokenKind::DotDot, Span::new(start, 2)))
            }
            _ => self.lex_ident(start),
        }
    }

    fn is_placeholder_start(&self) -> bool {
        // Check if { is followed by $, env., file., vars. (not just a block open)
        if self.pos + 1 >= self.bytes.len() {
            return false;
        }
        let next = self.bytes[self.pos + 1];
        if next == b'$' {
            return true;
        }
        // Check for known prefixes: env., file., vars.
        let rest = &self.source[self.pos + 1..];
        rest.starts_with("env.") || rest.starts_with("file.") || rest.starts_with("vars.")
    }

    fn lex_comment(&mut self, start: usize) -> Result<Token, LexError> {
        self.pos += 1; // skip #
        // Skip optional space after #
        if self.peek() == Some(b' ') {
            self.pos += 1;
        }
        let content_start = self.pos;
        while let Some(b) = self.peek() {
            if b == b'\n' {
                break;
            }
            self.pos += 1;
        }
        let content = self.source[content_start..self.pos].trim_end().to_string();
        Ok(Token::new(
            TokenKind::Comment(content),
            Span::new(start, self.pos - start),
        ))
    }

    fn lex_string(&mut self, start: usize) -> Result<Token, LexError> {
        self.pos += 1; // skip opening "
        // Build the string body as a byte buffer. The bytes we read either
        // come straight from the source (which is `&str`, guaranteed valid
        // UTF-8) or from ASCII escape replacements — both safe to pass to
        // `String::from_utf8`. The previous version pushed individual bytes
        // through `b as char`, which silently treated UTF-8 continuation
        // bytes as Latin-1 code points and mangled any non-ASCII content
        // (e.g. an em-dash `—` would arrive over the wire as `â` plus two
        // control chars).
        let mut value = Vec::<u8>::new();
        loop {
            let Some(b) = self.advance() else {
                return Err(LexError::UnterminatedString {
                    span: Span::new(start, 1).into(),
                });
            };
            match b {
                b'"' => break,
                b'\\' => {
                    let esc_pos = self.pos - 1;
                    let Some(esc) = self.advance() else {
                        return Err(LexError::UnterminatedString {
                            span: Span::new(start, 1).into(),
                        });
                    };
                    match esc {
                        b'n' => value.push(b'\n'),
                        b't' => value.push(b'\t'),
                        b'r' => value.push(b'\r'),
                        b'\\' => value.push(b'\\'),
                        b'"' => value.push(b'"'),
                        _ => {
                            return Err(LexError::InvalidEscape {
                                ch: esc as char,
                                span: Span::new(esc_pos, 2).into(),
                            });
                        }
                    }
                }
                b'\n' => {
                    return Err(LexError::UnterminatedString {
                        span: Span::new(start, 1).into(),
                    });
                }
                _ => value.push(b),
            }
        }
        let value = String::from_utf8(value).expect(
            "string body is built from valid-UTF-8 source bytes plus ASCII escape replacements",
        );
        Ok(Token::new(
            TokenKind::QuotedString(value),
            Span::new(start, self.pos - start),
        ))
    }

    fn lex_placeholder(&mut self, start: usize) -> Result<Token, LexError> {
        self.pos += 1; // skip {
        let content_start = self.pos;
        let mut depth = 1;
        while let Some(b) = self.peek() {
            match b {
                b'{' => {
                    depth += 1;
                    self.pos += 1;
                }
                b'}' => {
                    depth -= 1;
                    self.pos += 1;
                    if depth == 0 {
                        let content = self.source[content_start..self.pos - 1].to_string();
                        return Ok(Token::new(
                            TokenKind::Placeholder(content),
                            Span::new(start, self.pos - start),
                        ));
                    }
                }
                b'\n' => {
                    return Err(LexError::UnterminatedPlaceholder {
                        span: Span::new(start, 1).into(),
                    });
                }
                _ => self.pos += 1,
            }
        }
        Err(LexError::UnterminatedPlaceholder {
            span: Span::new(start, 1).into(),
        })
    }

    fn lex_ident(&mut self, start: usize) -> Result<Token, LexError> {
        while let Some(b) = self.peek() {
            if Self::is_ident_char(b) {
                // Check for .. which should NOT be consumed as part of ident
                if b == b'.' && self.peek_at(1) == Some(b'.') {
                    break;
                }
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = self.source[start..self.pos].to_string();
        Ok(Token::new(
            TokenKind::Ident(text),
            Span::new(start, self.pos - start),
        ))
    }

    fn is_ident_char(b: u8) -> bool {
        matches!(b,
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9'
            | b':' | b'/' | b'*' | b'?' | b'-' | b'.' | b'_'
            | b'+' | b'@'
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok_kinds(src: &str) -> Vec<TokenKind> {
        Lexer::tokenize(src)
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn empty_input() {
        assert_eq!(tok_kinds(""), vec![TokenKind::Eof]);
    }

    #[test]
    fn simple_block() {
        let kinds = tok_kinds("server { listen :9090 }");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident("server".into()),
                TokenKind::LBrace,
                TokenKind::Ident("listen".into()),
                TokenKind::Ident(":9090".into()),
                TokenKind::RBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn job_with_schedule() {
        let kinds = tok_kinds("job billing:invoice { every weekday at 02:00 }");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident("job".into()),
                TokenKind::Ident("billing:invoice".into()),
                TokenKind::LBrace,
                TokenKind::Ident("every".into()),
                TokenKind::Ident("weekday".into()),
                TokenKind::Ident("at".into()),
                TokenKind::Ident("02:00".into()),
                TokenKind::RBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn quoted_string() {
        let kinds = tok_kinds(r#"timezone "Europe/Vienna""#);
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident("timezone".into()),
                TokenKind::QuotedString("Europe/Vienna".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn placeholder() {
        let kinds = tok_kinds("auth token {env.CRONIQ_TOKEN}");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident("auth".into()),
                TokenKind::Ident("token".into()),
                TokenKind::Placeholder("env.CRONIQ_TOKEN".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn placeholder_with_default() {
        let kinds = tok_kinds("{$PORT:8080}");
        assert_eq!(
            kinds,
            vec![TokenKind::Placeholder("$PORT:8080".into()), TokenKind::Eof,]
        );
    }

    #[test]
    fn comment() {
        let kinds = tok_kinds("# This is a comment\nserver");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Comment("This is a comment".into()),
                TokenKind::Newline,
                TokenKind::Ident("server".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn semicolons() {
        let kinds = tok_kinds("level info; format json");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident("level".into()),
                TokenKind::Ident("info".into()),
                TokenKind::Semicolon,
                TokenKind::Ident("format".into()),
                TokenKind::Ident("json".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn dotdot_range() {
        let kinds = tok_kinds("window 02:00..06:00");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident("window".into()),
                TokenKind::Ident("02:00".into()),
                TokenKind::DotDot,
                TokenKind::Ident("06:00".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn string_escapes() {
        let tokens = Lexer::tokenize(r#""hello\nworld""#).unwrap();
        assert_eq!(
            tokens[0].kind,
            TokenKind::QuotedString("hello\nworld".into())
        );
    }

    #[test]
    fn unterminated_string() {
        let err = Lexer::tokenize(r#""hello"#).unwrap_err();
        assert!(matches!(err, LexError::UnterminatedString { .. }));
    }

    #[test]
    fn string_preserves_utf8_multibyte_chars() {
        // The previous byte-as-char implementation would split the
        // em-dash's three UTF-8 bytes (e2 80 94) into three Latin-1 code
        // points (â + 0x80 + 0x94) and re-encode them as UTF-8. This test
        // pins the correct round-trip.
        let tokens = Lexer::tokenize("\"Liveness ping — runs every minute\"").unwrap();
        match &tokens[0].kind {
            TokenKind::QuotedString(s) => {
                assert_eq!(s, "Liveness ping — runs every minute");
                assert_eq!(s.chars().count(), 33);
                assert!(s.contains('—'), "em-dash U+2014 must round-trip intact");
            }
            other => panic!("expected QuotedString, got {other:?}"),
        }

        // Wider coverage: emoji (4-byte UTF-8) + accented Latin (2-byte).
        let tokens = Lexer::tokenize("\"Düsseldorf 🚀\"").unwrap();
        match &tokens[0].kind {
            TokenKind::QuotedString(s) => assert_eq!(s, "Düsseldorf 🚀"),
            other => panic!("expected QuotedString, got {other:?}"),
        }
    }

    #[test]
    fn unterminated_placeholder() {
        let err = Lexer::tokenize("{env.FOO").unwrap_err();
        assert!(matches!(err, LexError::UnterminatedPlaceholder { .. }));
    }

    #[test]
    fn full_job_block() {
        let src = r#"
job billing:invoice-generate {
  every weekday at 02:00 {
    timezone "Europe/Vienna"
    calendar business-days
  }
  window 02:00..06:00
  runner { require billing; prefer eu-central }
  retry exponential { max_attempts 5; base 5s; cap 2m; jitter 0.25 }
  timeout 15m
}
"#;
        let tokens = Lexer::tokenize(src).unwrap();
        // Should not error
        assert!(tokens.last().unwrap().kind == TokenKind::Eof);
        // Check some key tokens
        let idents: Vec<&str> = tokens
            .iter()
            .filter_map(|t| match &t.kind {
                TokenKind::Ident(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert!(idents.contains(&"job"));
        assert!(idents.contains(&"billing:invoice-generate"));
        assert!(idents.contains(&"every"));
        assert!(idents.contains(&"weekday"));
        assert!(idents.contains(&"window"));
        assert!(idents.contains(&"02:00"));
    }

    #[test]
    fn span_tracking() {
        let tokens = Lexer::tokenize("server { }").unwrap();
        assert_eq!(tokens[0].span, Span::new(0, 6)); // "server"
        assert_eq!(tokens[1].span, Span::new(7, 1)); // "{"
        assert_eq!(tokens[2].span, Span::new(9, 1)); // "}"
    }

    #[test]
    fn import_with_glob() {
        let kinds = tok_kinds("import ./jobs/*.croniq");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident("import".into()),
                TokenKind::Ident("./jobs/*.croniq".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn vars_placeholder() {
        let kinds = tok_kinds("{vars.default_tz}");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Placeholder("vars.default_tz".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn monthly_schedule() {
        let kinds = tok_kinds("every 1st 15th of month at 10:00");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident("every".into()),
                TokenKind::Ident("1st".into()),
                TokenKind::Ident("15th".into()),
                TokenKind::Ident("of".into()),
                TokenKind::Ident("month".into()),
                TokenKind::Ident("at".into()),
                TokenKind::Ident("10:00".into()),
                TokenKind::Eof,
            ]
        );
    }
}
