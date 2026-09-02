use crate::ast::*;
use crate::lexer::{LexError, Lexer, Span, Token, TokenKind};
use miette::SourceSpan;

/// Parser error.
#[derive(Debug, Clone, thiserror::Error, miette::Diagnostic)]
pub enum ParseError {
    #[error("{message}")]
    #[diagnostic(code(croniq::parse::error))]
    General {
        message: String,
        #[label("{message}")]
        span: SourceSpan,
    },

    #[error("unexpected token: expected {expected}, got {got}")]
    #[diagnostic(code(croniq::parse::unexpected))]
    Unexpected {
        expected: String,
        got: String,
        #[label("here")]
        span: SourceSpan,
    },

    #[error("invalid job key '{key}': expected namespace:name or namespace:name:variant")]
    #[diagnostic(code(croniq::parse::invalid_job_key))]
    InvalidJobKey {
        key: String,
        #[label("invalid job key")]
        span: SourceSpan,
    },

    #[error("invalid time '{value}': expected HH:MM")]
    #[diagnostic(code(croniq::parse::invalid_time))]
    InvalidTime {
        value: String,
        #[label("invalid time")]
        span: SourceSpan,
    },

    #[error("invalid ordinal '{value}': expected 1st, 2nd, 3rd, ... or 'last'")]
    #[diagnostic(code(croniq::parse::invalid_ordinal))]
    InvalidOrdinal {
        value: String,
        #[label("invalid ordinal")]
        span: SourceSpan,
    },

    #[error(transparent)]
    #[diagnostic(transparent)]
    Lex(#[from] LexError),
}

/// Recursive-descent parser for Croniqfile.
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

/// Parse a bare schedule expression — the DSL text that would follow a job's
/// opening brace (`every 5 minutes`, `every day at 02:00`, `every Mon Fri at
/// 09:00`, `once at "…"`, `disabled`) — into its AST [`ScheduleKind`].
///
/// Wraps the expression in a synthetic job block and runs it through the real
/// parser, so it accepts exactly the Croniqfile schedule grammar (same wrapping
/// trick as `calendar_config_from_definition` in croniq-server). Used to
/// reconstruct a runtime schedule from a persisted `cron_expression` that holds
/// a canonical DSL line rather than interval shorthand — see
/// [`crate::schedule::CompiledSchedule::to_dsl`].
pub fn parse_schedule_expr(expr: &str) -> Result<ScheduleKind, ParseError> {
    let source = format!("job __sched__:__probe__ {{\n{expr}\n}}\n");
    let ast = Parser::parse(&source)?;
    ast.items
        .into_iter()
        .find_map(|item| match item {
            Item::Job(j) => j.schedule.map(|s| s.kind),
            _ => None,
        })
        .ok_or_else(|| ParseError::General {
            message: "expression did not contain a schedule".into(),
            span: Span::empty(0).into(),
        })
}

impl Parser {
    /// Parse a Croniqfile source string into an AST.
    pub fn parse(source: &str) -> Result<Croniqfile, ParseError> {
        let tokens = Lexer::tokenize(source)?;
        let mut parser = Parser { tokens, pos: 0 };
        parser.parse_croniqfile()
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn at_end(&self) -> bool {
        self.peek().kind == TokenKind::Eof
    }

    /// Skip newline tokens (they act as statement terminators but we need to ignore them
    /// in structural positions like before/after braces).
    fn skip_newlines(&mut self) {
        while self.peek().kind == TokenKind::Newline {
            self.pos += 1;
        }
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    fn expect_ident(&mut self, name: &str) -> Result<Span, ParseError> {
        let tok = self.peek().clone();
        if tok.is_ident(name) {
            self.advance();
            Ok(tok.span)
        } else {
            Err(ParseError::Unexpected {
                expected: format!("'{name}'"),
                got: format!("{}", tok.kind),
                span: tok.span.into(),
            })
        }
    }

    fn expect_lbrace(&mut self) -> Result<Span, ParseError> {
        self.skip_newlines();
        let tok = self.peek().clone();
        if tok.kind == TokenKind::LBrace {
            self.advance();
            Ok(tok.span)
        } else {
            Err(ParseError::Unexpected {
                expected: "'{'".into(),
                got: format!("{}", tok.kind),
                span: tok.span.into(),
            })
        }
    }

    fn expect_rbrace(&mut self) -> Result<Span, ParseError> {
        self.skip_newlines();
        let tok = self.peek().clone();
        if tok.kind == TokenKind::RBrace {
            self.advance();
            Ok(tok.span)
        } else {
            Err(ParseError::Unexpected {
                expected: "'}'".into(),
                got: format!("{}", tok.kind),
                span: tok.span.into(),
            })
        }
    }

    fn read_string_value(&mut self) -> Result<StringValue, ParseError> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Ident(s) => {
                let val = StringValue {
                    value: s.clone(),
                    quoted: false,
                    is_placeholder: false,
                    span: tok.span,
                };
                self.advance();
                Ok(val)
            }
            TokenKind::QuotedString(s) => {
                let val = StringValue {
                    value: s.clone(),
                    quoted: true,
                    is_placeholder: false,
                    span: tok.span,
                };
                self.advance();
                Ok(val)
            }
            TokenKind::Placeholder(s) => {
                let val = StringValue {
                    value: s.clone(),
                    quoted: false,
                    is_placeholder: true,
                    span: tok.span,
                };
                self.advance();
                Ok(val)
            }
            _ => Err(ParseError::Unexpected {
                expected: "identifier, string, or placeholder".into(),
                got: format!("{}", tok.kind),
                span: tok.span.into(),
            }),
        }
    }

    fn is_value_token(&self) -> bool {
        matches!(
            self.peek().kind,
            TokenKind::Ident(_) | TokenKind::QuotedString(_) | TokenKind::Placeholder(_)
        )
    }

    // ─── Top-level ───

    fn parse_croniqfile(&mut self) -> Result<Croniqfile, ParseError> {
        let start = self.peek().span;
        let mut items = Vec::new();

        loop {
            // Skip whitespace tokens
            while matches!(self.peek().kind, TokenKind::Semicolon | TokenKind::Newline) {
                self.advance();
            }
            if self.at_end() {
                break;
            }
            items.push(self.parse_item()?);
        }

        let end = self.peek().span;
        Ok(Croniqfile {
            items,
            span: start.merge(end),
        })
    }

    fn parse_item(&mut self) -> Result<Item, ParseError> {
        // Skip newlines and semicolons between items
        while matches!(self.peek().kind, TokenKind::Semicolon | TokenKind::Newline) {
            self.advance();
        }

        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Comment(_) => {
                let comment = self.parse_comment()?;
                Ok(Item::Comment(comment))
            }
            TokenKind::Ident(s) => match s.as_str() {
                "import" => Ok(Item::Import(self.parse_import()?)),
                "server" => Ok(Item::Server(self.parse_server()?)),
                "smtp" => Ok(Item::Smtp(self.parse_smtp()?)),
                "pull_api" => Ok(Item::PullApi(self.parse_pull_api()?)),
                "observability" => Ok(Item::Observability(self.parse_observability()?)),
                "mcp" => Ok(Item::Mcp(self.parse_mcp()?)),
                "oidc" => Ok(Item::Oidc(self.parse_oidc()?)),
                "auth" => Ok(Item::Auth(self.parse_auth()?)),
                "policy" => Ok(Item::Policy(self.parse_policy()?)),
                "alerts" => Ok(Item::Alerts(self.parse_alerts()?)),
                "vars" => Ok(Item::Vars(self.parse_vars()?)),
                "defaults" => Ok(Item::Defaults(self.parse_defaults()?)),
                "calendar" => Ok(Item::Calendar(self.parse_calendar()?)),
                "concurrency_group" => {
                    Ok(Item::ConcurrencyGroup(self.parse_concurrency_group()?))
                }
                "job" => Ok(Item::Job(self.parse_job()?)),
                other => Err(ParseError::General {
                    message: format!("unknown top-level block: '{other}'"),
                    span: tok.span.into(),
                }),
            },
            _ => Err(ParseError::Unexpected {
                expected:
                    "import, server, smtp, pull_api, observability, mcp, oidc, auth, policy, alerts, vars, defaults, calendar, concurrency_group, or job"
                        .into(),
                got: format!("{}", tok.kind),
                span: tok.span.into(),
            }),
        }
    }

    fn parse_comment(&mut self) -> Result<CommentNode, ParseError> {
        let tok = self.advance().clone();
        if let TokenKind::Comment(text) = &tok.kind {
            Ok(CommentNode {
                text: text.clone(),
                span: tok.span,
            })
        } else {
            unreachable!()
        }
    }

    // ─── Import ───

    fn parse_import(&mut self) -> Result<Import, ParseError> {
        let start = self.expect_ident("import")?;
        let path = self.read_string_value()?;
        Ok(Import {
            span: start.merge(path.span),
            path,
        })
    }

    // ─── Simple blocks ───

    fn parse_server(&mut self) -> Result<ServerBlock, ParseError> {
        let start = self.expect_ident("server")?;
        self.expect_lbrace()?;
        let directives = self.parse_directives_until_rbrace()?;
        let end = self.expect_rbrace()?;
        Ok(ServerBlock {
            directives,
            span: start.merge(end),
        })
    }

    fn parse_smtp(&mut self) -> Result<SmtpBlock, ParseError> {
        let start = self.expect_ident("smtp")?;
        self.expect_lbrace()?;
        let directives = self.parse_directives_until_rbrace()?;
        let end = self.expect_rbrace()?;
        Ok(SmtpBlock {
            directives,
            span: start.merge(end),
        })
    }

    fn parse_pull_api(&mut self) -> Result<PullApiBlock, ParseError> {
        let start = self.expect_ident("pull_api")?;
        self.expect_lbrace()?;
        let directives = self.parse_directives_until_rbrace()?;
        let end = self.expect_rbrace()?;
        Ok(PullApiBlock {
            directives,
            span: start.merge(end),
        })
    }

    fn parse_mcp(&mut self) -> Result<McpBlock, ParseError> {
        let start = self.expect_ident("mcp")?;
        self.expect_lbrace()?;
        let directives = self.parse_directives_until_rbrace()?;
        let end = self.expect_rbrace()?;
        Ok(McpBlock {
            directives,
            span: start.merge(end),
        })
    }

    fn parse_policy(&mut self) -> Result<PolicyBlock, ParseError> {
        let start = self.expect_ident("policy")?;
        self.expect_lbrace()?;
        let directives = self.parse_directives_until_rbrace()?;
        let end = self.expect_rbrace()?;
        Ok(PolicyBlock {
            directives,
            span: start.merge(end),
        })
    }

    fn parse_oidc(&mut self) -> Result<OidcBlock, ParseError> {
        let start = self.expect_ident("oidc")?;
        self.expect_lbrace()?;
        let directives = self.parse_directives_until_rbrace()?;
        let end = self.expect_rbrace()?;
        Ok(OidcBlock {
            directives,
            span: start.merge(end),
        })
    }

    fn parse_auth(&mut self) -> Result<AuthBlock, ParseError> {
        let start = self.expect_ident("auth")?;
        self.expect_lbrace()?;
        let mut sub_blocks = Vec::new();
        loop {
            self.skip_newlines();
            if self.peek().kind == TokenKind::RBrace || self.at_end() {
                break;
            }
            if let TokenKind::Comment(_) = self.peek().kind {
                self.advance();
                continue;
            }
            sub_blocks.push(self.parse_named_block()?);
        }
        let end = self.expect_rbrace()?;
        Ok(AuthBlock {
            sub_blocks,
            span: start.merge(end),
        })
    }

    /// `alerts { channel "name" { … } rule "name" { … } }`
    ///
    /// Each sub-block requires a string qualifier (the channel / rule
    /// name). The compile pass distinguishes channel vs rule by the
    /// outer `name` field and resolves channel-name references.
    fn parse_alerts(&mut self) -> Result<AlertsBlock, ParseError> {
        let start = self.expect_ident("alerts")?;
        self.expect_lbrace()?;
        let mut sub_blocks = Vec::new();
        loop {
            self.skip_newlines();
            if self.peek().kind == TokenKind::RBrace || self.at_end() {
                break;
            }
            if let TokenKind::Comment(_) = self.peek().kind {
                self.advance();
                continue;
            }
            let name = self.read_string_value()?;
            let name_span = name.span;
            // Require a qualifier — unnamed channels/rules are unreferenceable.
            if self.peek().kind == TokenKind::LBrace {
                return Err(ParseError::General {
                    message: format!(
                        "expected a string identifier after `{}` (e.g. `{0} \"name\" {{ … }}`)",
                        name.value
                    ),
                    span: name_span.into(),
                });
            }
            let qualifier = self.read_string_value()?;
            self.expect_lbrace()?;
            let directives = self.parse_directives_or_blocks_until_rbrace()?;
            let end = self.expect_rbrace()?;
            sub_blocks.push(NamedBlock {
                name,
                qualifier: Some(qualifier),
                directives,
                span: name_span.merge(end),
            });
        }
        let end = self.expect_rbrace()?;
        Ok(AlertsBlock {
            sub_blocks,
            span: start.merge(end),
        })
    }

    fn parse_observability(&mut self) -> Result<ObservabilityBlock, ParseError> {
        let start = self.expect_ident("observability")?;
        self.expect_lbrace()?;
        let mut sub_blocks = Vec::new();
        loop {
            self.skip_newlines();
            if self.peek().kind == TokenKind::RBrace || self.at_end() {
                break;
            }
            if let TokenKind::Comment(_) = self.peek().kind {
                self.advance();
                continue;
            }
            sub_blocks.push(self.parse_named_block()?);
        }
        let end = self.expect_rbrace()?;
        Ok(ObservabilityBlock {
            sub_blocks,
            span: start.merge(end),
        })
    }

    fn parse_vars(&mut self) -> Result<VarsBlock, ParseError> {
        let start = self.expect_ident("vars")?;
        self.expect_lbrace()?;
        let entries = self.parse_directives_until_rbrace()?;
        let end = self.expect_rbrace()?;
        Ok(VarsBlock {
            entries,
            span: start.merge(end),
        })
    }

    fn parse_defaults(&mut self) -> Result<DefaultsBlock, ParseError> {
        let start = self.expect_ident("defaults")?;
        self.expect_lbrace()?;
        let directives = self.parse_directives_or_blocks_until_rbrace()?;
        let end = self.expect_rbrace()?;
        Ok(DefaultsBlock {
            directives,
            span: start.merge(end),
        })
    }

    // ─── Calendar ───

    fn parse_calendar(&mut self) -> Result<CalendarBlock, ParseError> {
        let start = self.expect_ident("calendar")?;
        let name = self.read_string_value()?;
        self.expect_lbrace()?;

        let mut rules = Vec::new();
        loop {
            self.skip_newlines();
            if self.peek().kind == TokenKind::RBrace || self.at_end() {
                break;
            }
            if let TokenKind::Comment(_) = self.peek().kind {
                self.advance();
                continue;
            }
            if matches!(self.peek().kind, TokenKind::Semicolon) {
                self.advance();
                continue;
            }
            rules.push(self.parse_calendar_rule()?);
        }
        let end = self.expect_rbrace()?;

        Ok(CalendarBlock {
            name,
            rules,
            span: start.merge(end),
        })
    }

    // ─── Concurrency group ───

    /// `concurrency_group api-x { max_concurrent 1 }` (issue #546).
    ///
    /// Named like a `calendar` block and referenced the same way, but the body
    /// is a flat directive list, so it uses the generic directive parser. The
    /// key set is checked in `block_directives`, the semantics in `validate`.
    fn parse_concurrency_group(&mut self) -> Result<ConcurrencyGroupBlock, ParseError> {
        let start = self.expect_ident("concurrency_group")?;
        let name = self.read_string_value()?;
        self.expect_lbrace()?;
        let directives = self.parse_directives_until_rbrace()?;
        let end = self.expect_rbrace()?;
        Ok(ConcurrencyGroupBlock {
            name,
            directives,
            span: start.merge(end),
        })
    }

    fn parse_calendar_rule(&mut self) -> Result<CalendarRule, ParseError> {
        let tok = self.peek().clone();
        let kind = match tok.text() {
            "include" => CalendarRuleKind::Include,
            "exclude" => CalendarRuleKind::Exclude,
            "timezone" => {
                // timezone is a directive, not a rule — treat as special
                let key = self.read_string_value()?;
                let tz = self.read_string_value()?;
                return Ok(CalendarRule {
                    kind: CalendarRuleKind::Include,
                    rule_type: key,
                    args: vec![tz],
                    span: tok.span.merge(self.peek().span),
                });
            }
            other => {
                return Err(ParseError::General {
                    message: format!("expected 'include', 'exclude', or 'timezone', got '{other}'"),
                    span: tok.span.into(),
                });
            }
        };

        let start = self.read_string_value()?; // include/exclude
        let rule_type = self.read_string_value()?; // weekly/window/annual/monthly/yearly
        let rule_type_lower = rule_type.value.to_ascii_lowercase();

        let mut args = Vec::new();
        while self.is_value_token()
            && !self.peek().is_ident("include")
            && !self.peek().is_ident("exclude")
            && !self.peek().is_ident("timezone")
        {
            let first = self.read_string_value()?;
            if self.peek().kind == TokenKind::DotDot {
                self.advance();
                let second = self.read_string_value()?;
                // Range handling depends on the rule type:
                //   - `weekly`:  `Mon..Fri` → expand to 5 day tokens.
                //                Old parser dropped days silently, so this
                //                is a bugfix — 0.5.x calendars that used
                //                `weekly "Mon".."Fri"` actually only fired
                //                Mon/Fri.
                //   - `monthly`: `1..5` → expand to integer days.
                //   - `window`:  keep both endpoints; the runtime compiler
                //                splits on `..` itself.
                //   - others:    keep raw, pass through to the runtime.
                match rule_type_lower.as_str() {
                    "weekly" => {
                        if let (Some(s), Some(e)) =
                            (Weekday::parse(&first.value), Weekday::parse(&second.value))
                        {
                            // Emit lowercase full-name to match the rest
                            // of the AST. The runtime compiler accepts
                            // both forms; consistency simplifies tests.
                            for day in weekday_range(s, e) {
                                args.push(StringValue {
                                    value: day.as_str().to_string(),
                                    quoted: false,
                                    is_placeholder: false,
                                    span: first.span.merge(second.span),
                                });
                            }
                        } else {
                            args.push(first);
                            args.push(second);
                        }
                    }
                    "monthly" => match (
                        first.value.parse::<u32>().ok(),
                        second.value.parse::<u32>().ok(),
                    ) {
                        (Some(a), Some(b)) if a <= 31 && b <= 31 && a <= b => {
                            for d in a..=b {
                                args.push(StringValue {
                                    value: d.to_string(),
                                    quoted: false,
                                    is_placeholder: false,
                                    span: first.span.merge(second.span),
                                });
                            }
                        }
                        _ => {
                            args.push(first);
                            args.push(second);
                        }
                    },
                    _ => {
                        args.push(first);
                        args.push(second);
                    }
                }
            } else {
                // `weekly` group aliases (`weekday`/`weekend`) expand to
                // individual day tokens, same as ranges above — the
                // runtime compiler wants concrete day names (#356).
                // Placeholders stay verbatim for compile-time resolution.
                match Weekday::parse_group(&first.value) {
                    Some(group) if rule_type_lower == "weekly" && !first.is_placeholder => {
                        for day in group {
                            args.push(StringValue {
                                value: day.as_str().to_string(),
                                quoted: false,
                                is_placeholder: false,
                                span: first.span,
                            });
                        }
                    }
                    _ => args.push(first),
                }
            }
            if matches!(self.peek().kind, TokenKind::Semicolon | TokenKind::Newline) {
                self.advance();
                break;
            }
        }

        let span = start
            .span
            .merge(args.last().map(|a| a.span).unwrap_or(rule_type.span));
        Ok(CalendarRule {
            kind,
            rule_type,
            args,
            span,
        })
    }

    // ─── Job ───

    fn parse_job(&mut self) -> Result<JobBlock, ParseError> {
        let start = self.expect_ident("job")?;
        let key = self.parse_job_key()?;
        self.expect_lbrace()?;

        let mut schedule = None;
        let mut directives = Vec::new();

        loop {
            self.skip_newlines();
            if self.peek().kind == TokenKind::RBrace || self.at_end() {
                break;
            }
            if let TokenKind::Comment(text) = &self.peek().kind.clone() {
                directives.push(DirectiveOrBlock::Comment(CommentNode {
                    text: text.clone(),
                    span: self.peek().span,
                }));
                self.advance();
                continue;
            }
            if self.peek().kind == TokenKind::Semicolon {
                self.advance();
                continue;
            }

            // Try to parse schedule if we see schedule keywords
            if schedule.is_none() && self.is_schedule_start() {
                schedule = Some(self.parse_schedule()?);
            } else {
                directives.push(self.parse_directive_or_block()?);
            }
        }

        let end = self.expect_rbrace()?;

        Ok(JobBlock {
            key,
            schedule,
            directives,
            span: start.merge(end),
        })
    }

    fn parse_job_key(&mut self) -> Result<JobKey, ParseError> {
        let tok = self.peek().clone();
        let raw = match &tok.kind {
            TokenKind::Ident(s) => s.clone(),
            TokenKind::QuotedString(s) => s.clone(),
            _ => {
                return Err(ParseError::Unexpected {
                    expected: "job key".into(),
                    got: format!("{}", tok.kind),
                    span: tok.span.into(),
                });
            }
        };
        self.advance();

        let parts: Vec<&str> = raw.split(':').collect();
        if parts.len() < 2 || parts.len() > 3 {
            return Err(ParseError::InvalidJobKey {
                key: raw,
                span: tok.span.into(),
            });
        }

        Ok(JobKey {
            raw: raw.clone(),
            namespace: parts[0].to_string(),
            name: parts[1].to_string(),
            variant: parts.get(2).map(|s| s.to_string()),
            span: tok.span,
        })
    }

    fn is_schedule_start(&self) -> bool {
        let tok = self.peek();
        tok.is_ident("every")
            || tok.is_ident("once")
            || tok.is_ident("disabled")
            || tok.is_ident("ephemeral")
            || tok.is_ident("queued")
    }

    fn parse_schedule(&mut self) -> Result<ScheduleNode, ParseError> {
        let tok = self.peek().clone();

        // Optional execution-mode prefix: `ephemeral every …` / `queued every …`
        let mode = match tok.text() {
            "ephemeral" => {
                self.advance();
                Some(ScheduleMode::Ephemeral)
            }
            "queued" => {
                self.advance();
                Some(ScheduleMode::Queued)
            }
            _ => None,
        };

        let tok = self.peek().clone();
        let mut node = match tok.text() {
            "disabled" => {
                self.advance();
                ScheduleNode {
                    kind: ScheduleKind::Disabled,
                    mode: None,
                    options: vec![],
                    span: tok.span,
                }
            }
            "once" => self.parse_schedule_once()?,
            "every" => self.parse_schedule_every()?,
            _ => {
                return Err(ParseError::General {
                    message: "expected 'every', 'once', or 'disabled'".into(),
                    span: tok.span.into(),
                });
            }
        };

        node.mode = mode;
        Ok(node)
    }

    fn parse_schedule_once(&mut self) -> Result<ScheduleNode, ParseError> {
        let start = self.expect_ident("once")?;
        self.expect_ident("at")?;
        let datetime = self.read_string_value()?;
        let span = start.merge(datetime.span);

        Ok(ScheduleNode {
            kind: ScheduleKind::Once { at: datetime },
            mode: None,
            options: vec![],
            span,
        })
    }

    fn parse_schedule_every(&mut self) -> Result<ScheduleNode, ParseError> {
        let start = self.expect_ident("every")?;

        // Look ahead to determine schedule type
        let next = self.peek().clone();

        // Check for day names or "weekday"/"weekend"
        if self.is_day_name(next.text()) {
            return self.parse_schedule_weekdays(start);
        }

        // Check for "day"
        if next.is_ident("day") {
            return self.parse_schedule_daily(start);
        }

        // Check for ordinals (1st, 2nd, ..., last)
        if self.is_ordinal(next.text()) {
            return self.parse_schedule_monthly(start);
        }

        // Must be interval: every N unit
        self.parse_schedule_interval(start)
    }

    fn parse_schedule_interval(&mut self, start: Span) -> Result<ScheduleNode, ParseError> {
        let count_tok = self.peek().clone();
        let text = count_tok.text();

        // Accept both the verbose form (`every 15 minutes`) and the compact
        // suffixed form (`every 15m`, `every 30s`, `every 2h`). The compact
        // form is already accepted on `timeout`, so the asymmetry was
        // surprising — see issue #216.
        let (count, unit, end_span) = if let Some((c, u)) = parse_compact_interval(text) {
            self.advance();
            (c, u, count_tok.span)
        } else {
            let count: u32 = text.parse().map_err(|_| ParseError::General {
                message: format!(
                    "expected number or compact duration (e.g. '15', '15m', '30s', '2h'), got '{text}'"
                ),
                span: count_tok.span.into(),
            })?;
            self.advance();

            let unit_tok = self.peek().clone();
            let unit = match unit_tok.text() {
                "seconds" | "second" => IntervalUnit::Seconds,
                "minutes" | "minute" => IntervalUnit::Minutes,
                "hours" | "hour" => IntervalUnit::Hours,
                other => {
                    return Err(ParseError::General {
                        message: format!(
                            "expected 'seconds', 'minutes', or 'hours', got '{other}'"
                        ),
                        span: unit_tok.span.into(),
                    });
                }
            };
            self.advance();
            (count, unit, unit_tok.span)
        };

        let (options, end) = self.parse_optional_schedule_block(end_span)?;
        Ok(ScheduleNode {
            kind: ScheduleKind::Interval { count, unit },
            mode: None,
            options,
            span: start.merge(end),
        })
    }

    fn parse_schedule_daily(&mut self, start: Span) -> Result<ScheduleNode, ParseError> {
        self.expect_ident("day")?;
        self.expect_ident("at")?;
        let time = self.parse_time()?;

        let (options, end) = self.parse_optional_schedule_block(time.span)?;
        Ok(ScheduleNode {
            kind: ScheduleKind::Daily { time },
            mode: None,
            options,
            span: start.merge(end),
        })
    }

    fn parse_schedule_weekdays(&mut self, start: Span) -> Result<ScheduleNode, ParseError> {
        let mut days = Vec::new();

        // Collect day tokens, expanding `Mon..Fri` ranges and the
        // `weekday`/`weekend` aliases. Plain day idents push the
        // single day. Ranges only chain off specific weekdays —
        // `weekday..Fri` would be ambiguous and is rejected.
        while self.is_day_name(self.peek().text()) {
            let tok = self.peek().clone();
            match Weekday::parse_group(tok.text()) {
                Some(group) => {
                    days.extend(group.iter().copied());
                    self.advance();
                }
                None => {
                    let start_day = Weekday::parse(tok.text()).expect("is_day_name guarded");
                    self.advance();
                    if self.peek().kind == TokenKind::DotDot {
                        // Range form: `Mon..Fri`. The lexer emits the
                        // intermediate token as `..` so we just need
                        // the next day name.
                        let dotdot_span = self.peek().span;
                        self.advance();
                        let end_tok = self.peek().clone();
                        let Some(end_day) = Weekday::parse(end_tok.text()) else {
                            return Err(ParseError::General {
                                message: format!(
                                    "expected weekday name after '..', got '{}'",
                                    end_tok.text()
                                ),
                                span: dotdot_span.merge(end_tok.span).into(),
                            });
                        };
                        self.advance();
                        days.extend(weekday_range(start_day, end_day));
                    } else {
                        days.push(start_day);
                    }
                }
            }
        }

        self.expect_ident("at")?;
        let time = self.parse_time()?;

        let (options, end) = self.parse_optional_schedule_block(time.span)?;
        Ok(ScheduleNode {
            kind: ScheduleKind::Weekdays { days, time },
            mode: None,
            options,
            span: start.merge(end),
        })
    }

    fn parse_schedule_monthly(&mut self, start: Span) -> Result<ScheduleNode, ParseError> {
        let mut ordinals = Vec::new();

        while self.is_ordinal(self.peek().text()) {
            ordinals.push(self.parse_ordinal()?);
        }

        self.expect_ident("of")?;
        self.expect_ident("month")?;
        self.expect_ident("at")?;
        let time = self.parse_time()?;

        let (options, end) = self.parse_optional_schedule_block(time.span)?;
        Ok(ScheduleNode {
            kind: ScheduleKind::Monthly { ordinals, time },
            mode: None,
            options,
            span: start.merge(end),
        })
    }

    fn parse_optional_schedule_block(
        &mut self,
        last_span: Span,
    ) -> Result<(Vec<Directive>, Span), ParseError> {
        if self.peek().kind == TokenKind::LBrace {
            self.expect_lbrace()?;
            let options = self.parse_directives_until_rbrace()?;
            let end = self.expect_rbrace()?;
            Ok((options, end))
        } else {
            Ok((vec![], last_span))
        }
    }

    fn parse_time(&mut self) -> Result<TimeValue, ParseError> {
        let tok = self.peek().clone();
        let raw = tok.text().to_string();

        let parts: Vec<&str> = raw.split(':').collect();
        if parts.len() != 2 {
            return Err(ParseError::InvalidTime {
                value: raw,
                span: tok.span.into(),
            });
        }

        let hour: u8 = parts[0].parse().map_err(|_| ParseError::InvalidTime {
            value: raw.clone(),
            span: tok.span.into(),
        })?;
        let minute: u8 = parts[1].parse().map_err(|_| ParseError::InvalidTime {
            value: raw.clone(),
            span: tok.span.into(),
        })?;

        if hour > 23 || minute > 59 {
            return Err(ParseError::InvalidTime {
                value: raw,
                span: tok.span.into(),
            });
        }

        self.advance();

        Ok(TimeValue {
            hour,
            minute,
            raw,
            span: tok.span,
        })
    }

    fn is_day_name(&self, s: &str) -> bool {
        // Aliases (`weekday`, `weekend`) stay full-length only — they
        // expand to multiple days, so a 3-letter form would be
        // ambiguous (`wee`?). Specific weekdays accept the canonical
        // full name plus the 3-letter abbreviation, case-insensitive.
        Weekday::parse_token(s).is_some()
    }

    fn is_ordinal(&self, s: &str) -> bool {
        if s == "last" {
            return true;
        }
        // Match 1st, 2nd, 3rd, 4th, ..., 31st
        s.ends_with("st") || s.ends_with("nd") || s.ends_with("rd") || s.ends_with("th")
    }

    fn parse_ordinal(&mut self) -> Result<MonthOrdinal, ParseError> {
        let tok = self.peek().clone();
        let text = tok.text();

        if text == "last" {
            self.advance();
            return Ok(MonthOrdinal::Last);
        }

        // Strip suffix and parse number
        let num_str = text
            .strip_suffix("st")
            .or_else(|| text.strip_suffix("nd"))
            .or_else(|| text.strip_suffix("rd"))
            .or_else(|| text.strip_suffix("th"))
            .ok_or_else(|| ParseError::InvalidOrdinal {
                value: text.to_string(),
                span: tok.span.into(),
            })?;

        let num: u8 = num_str.parse().map_err(|_| ParseError::InvalidOrdinal {
            value: text.to_string(),
            span: tok.span.into(),
        })?;

        if num == 0 || num > 31 {
            return Err(ParseError::InvalidOrdinal {
                value: text.to_string(),
                span: tok.span.into(),
            });
        }

        self.advance();
        Ok(MonthOrdinal::Day(num))
    }

    // ─── Generic directive/block parsing ───

    fn parse_directives_until_rbrace(&mut self) -> Result<Vec<Directive>, ParseError> {
        let mut directives = Vec::new();
        loop {
            self.skip_newlines();
            if self.peek().kind == TokenKind::RBrace || self.at_end() {
                break;
            }
            if let TokenKind::Comment(_) = self.peek().kind {
                self.advance();
                continue;
            }
            if self.peek().kind == TokenKind::Semicolon {
                self.advance();
                continue;
            }
            directives.push(self.parse_directive()?);
        }
        Ok(directives)
    }

    fn parse_directive(&mut self) -> Result<Directive, ParseError> {
        let key = self.read_string_value()?;
        let mut args = Vec::new();

        while self.is_value_token() {
            args.push(self.read_string_value()?);
            // Handle dotdot in ranges
            if self.peek().kind == TokenKind::DotDot {
                self.advance();
                let after = self.read_string_value()?;
                let range_val = format!("{}..{}", args.last().unwrap().value, after.value);
                let last = args.last_mut().unwrap();
                last.value = range_val;
                last.span = last.span.merge(after.span);
            }
        }

        // Consume statement terminator (semicolon or newline)
        while matches!(self.peek().kind, TokenKind::Semicolon | TokenKind::Newline) {
            self.advance();
        }

        let span = key
            .span
            .merge(args.last().map(|a| a.span).unwrap_or(key.span));
        Ok(Directive { key, args, span })
    }

    fn parse_directives_or_blocks_until_rbrace(
        &mut self,
    ) -> Result<Vec<DirectiveOrBlock>, ParseError> {
        let mut items = Vec::new();
        loop {
            self.skip_newlines();
            if self.peek().kind == TokenKind::RBrace || self.at_end() {
                break;
            }
            items.push(self.parse_directive_or_block()?);
        }
        Ok(items)
    }

    fn parse_directive_or_block(&mut self) -> Result<DirectiveOrBlock, ParseError> {
        // Skip statement terminators
        while matches!(self.peek().kind, TokenKind::Semicolon | TokenKind::Newline) {
            self.advance();
        }

        if let TokenKind::Comment(text) = &self.peek().kind.clone() {
            let node = CommentNode {
                text: text.clone(),
                span: self.peek().span,
            };
            self.advance();
            return Ok(DirectiveOrBlock::Comment(node));
        }

        // Look ahead: is this a directive with a block?
        // Pattern: ident [qualifier] {
        let saved_pos = self.pos;
        let name = self.read_string_value()?;
        let name_span = name.span;

        // Check if next token is { (block) or a qualifier followed by {
        if self.peek().kind == TokenKind::LBrace {
            self.expect_lbrace()?;
            let directives = self.parse_directives_or_blocks_until_rbrace()?;
            let end = self.expect_rbrace()?;
            return Ok(DirectiveOrBlock::Block(NamedBlock {
                name,
                qualifier: None,
                directives,
                span: name_span.merge(end),
            }));
        }

        // Check if: name qualifier {
        if self.is_value_token() {
            let maybe_qualifier = self.read_string_value()?;
            if self.peek().kind == TokenKind::LBrace {
                self.expect_lbrace()?;
                let directives = self.parse_directives_or_blocks_until_rbrace()?;
                let end = self.expect_rbrace()?;
                return Ok(DirectiveOrBlock::Block(NamedBlock {
                    name,
                    qualifier: Some(maybe_qualifier),
                    directives,
                    span: name_span.merge(end),
                }));
            }
            // Not a block — rewind and parse as directive
            self.pos = saved_pos;
        } else {
            // Simple directive with no args, rewind
            self.pos = saved_pos;
        }

        // Parse as simple directive
        let directive = self.parse_directive()?;
        Ok(DirectiveOrBlock::Directive(directive))
    }

    fn parse_named_block(&mut self) -> Result<NamedBlock, ParseError> {
        let name = self.read_string_value()?;
        let name_span = name.span;
        self.expect_lbrace()?;
        let directives = self.parse_directives_or_blocks_until_rbrace()?;
        let end = self.expect_rbrace()?;
        Ok(NamedBlock {
            name,
            qualifier: None,
            directives,
            span: name_span.merge(end),
        })
    }
}

/// Expand a `start..end` weekday range into the inclusive list of days
/// it covers, walking forward through the week starting at `start`.
/// Wraps around at Sunday so e.g. `Sat..Mon` yields `[Sat, Sun, Mon]`.
/// The Croniqfile DSL does not document direction, so we accept any
/// pair without erroring — wrap-around matches what users typing
/// `Fri..Mon` for "long weekend" would expect.
/// Parse the compact duration form used on `every` (and elsewhere):
/// `15m`, `30s`, `2h`. Returns `None` if the input is not a compact form
/// (so the parser can fall back to the verbose `15 minutes` path or
/// surface a parse error from the trailing parts).
fn parse_compact_interval(text: &str) -> Option<(u32, IntervalUnit)> {
    let (num_part, unit) = if let Some(rest) = text.strip_suffix('s') {
        (rest, IntervalUnit::Seconds)
    } else if let Some(rest) = text.strip_suffix('m') {
        (rest, IntervalUnit::Minutes)
    } else {
        // Hours is the last recognised suffix; anything else is not a
        // compact form. `?` returns `None` here, which clippy's
        // `question_mark` lint (tightened in Rust 1.97) requires over a
        // trailing `else { return None }`.
        let rest = text.strip_suffix('h')?;
        (rest, IntervalUnit::Hours)
    };
    if num_part.is_empty() {
        return None;
    }
    let count: u32 = num_part.parse().ok()?;
    Some((count, unit))
}

fn weekday_range(start: Weekday, end: Weekday) -> Vec<Weekday> {
    const ORDER: [Weekday; 7] = [
        Weekday::Monday,
        Weekday::Tuesday,
        Weekday::Wednesday,
        Weekday::Thursday,
        Weekday::Friday,
        Weekday::Saturday,
        Weekday::Sunday,
    ];
    let start_idx = ORDER.iter().position(|d| *d == start).unwrap();
    let end_idx = ORDER.iter().position(|d| *d == end).unwrap();
    let mut out = Vec::new();
    let mut i = start_idx;
    loop {
        out.push(ORDER[i]);
        if i == end_idx {
            break;
        }
        i = (i + 1) % 7;
        // Safety net for an impossible (start_idx == end_idx but loop
        // logic somehow misses): cap at 7 entries.
        if out.len() >= 7 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty() {
        let ast = Parser::parse("").unwrap();
        assert!(ast.items.is_empty());
    }

    #[test]
    fn parse_server_block() {
        let ast = Parser::parse("server { listen :9090; db sqlite }").unwrap();
        assert!(matches!(ast.items[0], Item::Server(_)));
        if let Item::Server(ref s) = ast.items[0] {
            assert_eq!(s.directives.len(), 2);
            assert_eq!(s.directives[0].key.value, "listen");
            assert_eq!(s.directives[0].args[0].value, ":9090");
        }
    }

    #[test]
    fn parse_smtp_block() {
        let ast = Parser::parse(r#"smtp { host "mail.example.com"; port 587; security starttls }"#)
            .unwrap();
        assert!(matches!(ast.items[0], Item::Smtp(_)));
        if let Item::Smtp(ref s) = ast.items[0] {
            assert_eq!(s.directives.len(), 3);
            assert_eq!(s.directives[0].key.value, "host");
            assert_eq!(s.directives[0].args[0].value, "mail.example.com");
            assert_eq!(s.directives[1].key.value, "port");
        }
    }

    #[test]
    fn parse_job_with_interval() {
        let ast = Parser::parse("job etl:sync { every 15 minutes; timeout 10m }").unwrap();
        if let Item::Job(ref j) = ast.items[0] {
            assert_eq!(j.key.namespace, "etl");
            assert_eq!(j.key.name, "sync");
            let sched = j.schedule.as_ref().unwrap();
            assert!(matches!(
                sched.kind,
                ScheduleKind::Interval {
                    count: 15,
                    unit: IntervalUnit::Minutes
                }
            ));
        } else {
            panic!("expected Job");
        }
    }

    #[test]
    fn parse_job_with_compact_interval() {
        for (src, want_count, want_unit) in [
            ("job a:b { every 1m }", 1u32, IntervalUnit::Minutes),
            ("job a:b { every 30s }", 30, IntervalUnit::Seconds),
            ("job a:b { every 2h }", 2, IntervalUnit::Hours),
        ] {
            let ast = Parser::parse(src).unwrap_or_else(|e| panic!("{src}: {e:?}"));
            if let Item::Job(ref j) = ast.items[0] {
                let sched = j.schedule.as_ref().unwrap();
                match sched.kind {
                    ScheduleKind::Interval { count, unit } => {
                        assert_eq!(count, want_count, "src={src}");
                        assert_eq!(unit, want_unit, "src={src}");
                    }
                    _ => panic!("expected Interval for {src}"),
                }
            } else {
                panic!("expected Job for {src}");
            }
        }
    }

    #[test]
    fn parse_job_with_weekday_schedule() {
        let ast = Parser::parse(
            r#"job billing:invoice {
                every weekday at 02:00
                timeout 15m
            }"#,
        )
        .unwrap();
        if let Item::Job(ref j) = ast.items[0] {
            let sched = j.schedule.as_ref().unwrap();
            if let ScheduleKind::Weekdays { ref days, ref time } = sched.kind {
                assert_eq!(days.len(), 5);
                assert_eq!(time.hour, 2);
                assert_eq!(time.minute, 0);
            } else {
                panic!("expected Weekdays schedule");
            }
        }
    }

    #[test]
    fn parse_schedule_weekdays_3letter_case_insensitive() {
        // 3-letter forms — same days as the full-name version. Mixing
        // cases on purpose to lock the case-insensitive path.
        let ast = Parser::parse("job demo:k { every Mon TUE wed at 09:00 }").unwrap();
        if let Item::Job(ref j) = ast.items[0] {
            if let ScheduleKind::Weekdays { ref days, .. } = j.schedule.as_ref().unwrap().kind {
                assert_eq!(
                    *days,
                    vec![Weekday::Monday, Weekday::Tuesday, Weekday::Wednesday]
                );
            } else {
                panic!("expected Weekdays");
            }
        }
    }

    #[test]
    fn parse_schedule_weekdays_range() {
        // `Mon..Fri` should expand to all five business days.
        let ast = Parser::parse("job demo:k { every Mon..Fri at 09:00 }").unwrap();
        if let Item::Job(ref j) = ast.items[0] {
            if let ScheduleKind::Weekdays { ref days, .. } = j.schedule.as_ref().unwrap().kind {
                assert_eq!(
                    *days,
                    vec![
                        Weekday::Monday,
                        Weekday::Tuesday,
                        Weekday::Wednesday,
                        Weekday::Thursday,
                        Weekday::Friday,
                    ]
                );
            } else {
                panic!("expected Weekdays");
            }
        }
    }

    #[test]
    fn parse_schedule_weekdays_range_wraps_sunday() {
        // `Fri..Mon` covers a long-weekend duty rotation. Wrap-around
        // through Sunday is the expected behaviour.
        let ast = Parser::parse("job demo:k { every Fri..Mon at 09:00 }").unwrap();
        if let Item::Job(ref j) = ast.items[0] {
            if let ScheduleKind::Weekdays { ref days, .. } = j.schedule.as_ref().unwrap().kind {
                assert_eq!(
                    *days,
                    vec![
                        Weekday::Friday,
                        Weekday::Saturday,
                        Weekday::Sunday,
                        Weekday::Monday,
                    ]
                );
            } else {
                panic!("expected Weekdays");
            }
        }
    }

    #[test]
    fn parse_schedule_weekdays_mixed_singletons_and_range() {
        // Day list + range chained together. Resulting vec preserves
        // order, including any duplicates the user produced.
        let ast = Parser::parse("job demo:k { every Mon Wed..Fri Sun at 09:00 }").unwrap();
        if let Item::Job(ref j) = ast.items[0] {
            if let ScheduleKind::Weekdays { ref days, .. } = j.schedule.as_ref().unwrap().kind {
                assert_eq!(
                    *days,
                    vec![
                        Weekday::Monday,
                        Weekday::Wednesday,
                        Weekday::Thursday,
                        Weekday::Friday,
                        Weekday::Sunday,
                    ]
                );
            } else {
                panic!("expected Weekdays");
            }
        }
    }

    #[test]
    fn parse_job_with_daily_schedule_and_options() {
        let ast = Parser::parse(
            r#"job billing:invoice {
                every day at 02:00 {
                    calendar business-days
                    not_before 2026-01-01T00:00:00Z
                }
                timeout 15m
            }"#,
        )
        .unwrap();
        if let Item::Job(ref j) = ast.items[0] {
            let sched = j.schedule.as_ref().unwrap();
            assert!(matches!(sched.kind, ScheduleKind::Daily { .. }));
            assert_eq!(sched.options.len(), 2);
            assert_eq!(sched.options[0].key.value, "calendar");
        }
    }

    #[test]
    fn parse_job_with_monthly_schedule() {
        let ast =
            Parser::parse("job reports:summary { every 1st 15th of month at 06:00; timeout 1h }")
                .unwrap();
        if let Item::Job(ref j) = ast.items[0] {
            let sched = j.schedule.as_ref().unwrap();
            if let ScheduleKind::Monthly {
                ref ordinals,
                ref time,
            } = sched.kind
            {
                assert_eq!(ordinals.len(), 2);
                assert!(matches!(ordinals[0], MonthOrdinal::Day(1)));
                assert!(matches!(ordinals[1], MonthOrdinal::Day(15)));
                assert_eq!(time.hour, 6);
            } else {
                panic!("expected Monthly schedule");
            }
        }
    }

    #[test]
    fn parse_job_once() {
        let ast = Parser::parse("job migration:v2 { once at 2026-04-01T03:00:00Z; timeout 30m }")
            .unwrap();
        if let Item::Job(ref j) = ast.items[0] {
            let sched = j.schedule.as_ref().unwrap();
            if let ScheduleKind::Once { ref at } = sched.kind {
                assert_eq!(at.value, "2026-04-01T03:00:00Z");
            } else {
                panic!("expected Once schedule");
            }
        }
    }

    #[test]
    fn parse_job_disabled() {
        let ast = Parser::parse("job legacy:old { disabled; timeout 5m }").unwrap();
        if let Item::Job(ref j) = ast.items[0] {
            let sched = j.schedule.as_ref().unwrap();
            assert!(matches!(sched.kind, ScheduleKind::Disabled));
        }
    }

    #[test]
    fn parse_calendar() {
        let ast = Parser::parse(
            r#"calendar business-days {
                timezone "Europe/Vienna"
                include weekly monday tuesday wednesday thursday friday
                exclude annual 01-01 12-25 12-26
            }"#,
        )
        .unwrap();
        if let Item::Calendar(ref c) = ast.items[0] {
            assert_eq!(c.name.value, "business-days");
            // timezone + include + exclude = 3 rules
            assert_eq!(c.rules.len(), 3);
        }
    }

    #[test]
    fn parse_calendar_weekly_range_expands() {
        // `Mon..Fri` should expand to all five days at parse time.
        // Pre-PR-D this lost Tue/Wed/Thu silently.
        let ast = Parser::parse(r#"calendar biz { include weekly "Mon".."Fri" }"#).unwrap();
        let Item::Calendar(ref c) = ast.items[0] else {
            panic!("expected calendar")
        };
        // Find the include rule — the timezone synthetic Include is
        // skipped here since this calendar has no timezone directive.
        let rule = c
            .rules
            .iter()
            .find(|r| r.rule_type.value == "weekly")
            .expect("weekly rule");
        let arg_values: Vec<&str> = rule.args.iter().map(|a| a.value.as_str()).collect();
        assert_eq!(
            arg_values,
            vec!["monday", "tuesday", "wednesday", "thursday", "friday"]
        );
    }

    #[test]
    fn parse_calendar_weekly_3letter_unquoted() {
        // 3-letter unquoted forms accepted now that Weekday::parse is
        // case-insensitive and the calendar parser stores raw idents.
        let ast = Parser::parse(r#"calendar biz { include weekly Mon Wed Fri }"#).unwrap();
        let Item::Calendar(ref c) = ast.items[0] else {
            panic!()
        };
        let rule = c
            .rules
            .iter()
            .find(|r| r.rule_type.value == "weekly")
            .unwrap();
        // Args are the raw tokens — the runtime compiler does the
        // `parse_weekday` step. We only check round-trip through the
        // parser here.
        assert_eq!(rule.args.len(), 3);
    }

    #[test]
    fn parse_calendar_monthly_range_expands() {
        // `monthly 1..5` → individual integer days.
        let ast = Parser::parse(r#"calendar biz { include monthly 1..5 }"#).unwrap();
        let Item::Calendar(ref c) = ast.items[0] else {
            panic!()
        };
        let rule = c
            .rules
            .iter()
            .find(|r| r.rule_type.value == "monthly")
            .unwrap();
        let values: Vec<&str> = rule.args.iter().map(|a| a.value.as_str()).collect();
        assert_eq!(values, vec!["1", "2", "3", "4", "5"]);
    }

    #[test]
    fn parse_calendar_window_keeps_endpoints() {
        // `window` rules are time-of-day; the runtime expects the two
        // endpoints, so the parser must NOT expand them.
        let ast = Parser::parse(r#"calendar biz { include window "08:00".."18:00" }"#).unwrap();
        let Item::Calendar(ref c) = ast.items[0] else {
            panic!()
        };
        let rule = c
            .rules
            .iter()
            .find(|r| r.rule_type.value == "window")
            .unwrap();
        let values: Vec<&str> = rule.args.iter().map(|a| a.value.as_str()).collect();
        assert_eq!(values, vec!["08:00", "18:00"]);
    }

    fn weekly_rule_args(src: &str) -> Vec<String> {
        let ast = Parser::parse(src).unwrap();
        let Item::Calendar(ref c) = ast.items[0] else {
            panic!("expected calendar")
        };
        c.rules
            .iter()
            .find(|r| r.rule_type.value == "weekly")
            .expect("weekly rule")
            .args
            .iter()
            .map(|a| a.value.clone())
            .collect()
    }

    #[test]
    fn parse_calendar_weekly_weekday_alias_expands() {
        // #356: `fmt` and the cron converter emit the `weekday` alias;
        // it must expand to concrete days like ranges do, or the
        // runtime compiler rejects the calendar.
        assert_eq!(
            weekly_rule_args("calendar biz { include weekly weekday }"),
            vec!["monday", "tuesday", "wednesday", "thursday", "friday"]
        );

        // Every expanded arg carries the alias token's span.
        let ast = Parser::parse("calendar biz { include weekly weekday }").unwrap();
        let Item::Calendar(ref c) = ast.items[0] else {
            panic!()
        };
        let rule = c
            .rules
            .iter()
            .find(|r| r.rule_type.value == "weekly")
            .unwrap();
        assert!(rule.args.iter().all(|a| a.span == rule.args[0].span));
    }

    #[test]
    fn parse_calendar_weekly_weekend_alias_expands() {
        assert_eq!(
            weekly_rule_args("calendar biz { include weekly weekend }"),
            vec!["saturday", "sunday"]
        );
    }

    #[test]
    fn parse_calendar_weekly_quoted_alias_expands() {
        // Quoted form, consistent with the quoted range path.
        assert_eq!(
            weekly_rule_args(r#"calendar biz { include weekly "weekday" }"#),
            vec!["monday", "tuesday", "wednesday", "thursday", "friday"]
        );
    }

    #[test]
    fn parse_calendar_weekly_placeholder_not_expanded() {
        // Placeholders resolve at compile time — the parser must not
        // touch them (the runtime compiler expands aliases itself).
        let ast = Parser::parse("calendar biz { include weekly {vars.days} }").unwrap();
        let Item::Calendar(ref c) = ast.items[0] else {
            panic!()
        };
        let rule = c
            .rules
            .iter()
            .find(|r| r.rule_type.value == "weekly")
            .unwrap();
        assert_eq!(rule.args.len(), 1);
        assert!(rule.args[0].is_placeholder);
        assert_eq!(rule.args[0].value, "vars.days");
    }

    #[test]
    fn parse_calendar_alias_only_for_weekly() {
        // No cross-rule expansion: `monthly weekday` is nonsense and
        // stays verbatim for the runtime compiler to reject.
        let ast = Parser::parse("calendar biz { include monthly weekday }").unwrap();
        let Item::Calendar(ref c) = ast.items[0] else {
            panic!()
        };
        let rule = c
            .rules
            .iter()
            .find(|r| r.rule_type.value == "monthly")
            .unwrap();
        let values: Vec<&str> = rule.args.iter().map(|a| a.value.as_str()).collect();
        assert_eq!(values, vec!["weekday"]);
    }

    #[test]
    fn parse_schedule_weekday_alias_expands() {
        // Regression guard for the parse_schedule_weekdays refactor.
        let ast =
            Parser::parse("job a:b {\n  every weekday at 09:00\n  runner { require default }\n}")
                .unwrap();
        let Item::Job(ref job) = ast.items[0] else {
            panic!("expected job")
        };
        let ScheduleKind::Weekdays { ref days, .. } = job.schedule.as_ref().unwrap().kind else {
            panic!("expected weekdays schedule")
        };
        assert_eq!(days.len(), 5);
        assert_eq!(days[0], Weekday::Monday);
        assert_eq!(days[4], Weekday::Friday);
    }

    #[test]
    fn parse_defaults() {
        let ast = Parser::parse(
            r#"defaults {
                timezone Europe/Vienna
                retry exponential { max_attempts 3; base 2s; cap 30s; jitter 0.25 }
                timeout 5m
            }"#,
        )
        .unwrap();
        assert!(matches!(ast.items[0], Item::Defaults(_)));
    }

    #[test]
    fn parse_import() {
        let ast = Parser::parse("import ./jobs/*.croniq").unwrap();
        if let Item::Import(ref i) = ast.items[0] {
            assert_eq!(i.path.value, "./jobs/*.croniq");
        }
    }

    #[test]
    fn parse_job_with_window() {
        let ast = Parser::parse(
            r#"job etl:heavy {
                every day at 02:00
                window 02:00..06:00
                timeout 3h
            }"#,
        )
        .unwrap();
        if let Item::Job(ref j) = ast.items[0] {
            // window should be in directives
            let window = j
                .directives
                .iter()
                .find(|d| matches!(d, DirectiveOrBlock::Directive(d) if d.key.value == "window"));
            assert!(window.is_some());
        }
    }

    #[test]
    fn parse_job_with_runner_block() {
        let ast = Parser::parse(
            r#"job ops:check {
                every 5 minutes
                runner { require health-check; require eu-west }
                timeout 30s
            }"#,
        )
        .unwrap();
        if let Item::Job(ref j) = ast.items[0] {
            let runner = j
                .directives
                .iter()
                .find(|d| matches!(d, DirectiveOrBlock::Block(b) if b.name.value == "runner"));
            assert!(runner.is_some());
        }
    }

    #[test]
    fn parse_job_key_with_variant() {
        let ast = Parser::parse("job ops:health:eu-west { every 5 minutes; timeout 30s }").unwrap();
        if let Item::Job(ref j) = ast.items[0] {
            assert_eq!(j.key.namespace, "ops");
            assert_eq!(j.key.name, "health");
            assert_eq!(j.key.variant.as_deref(), Some("eu-west"));
        }
    }

    #[test]
    fn invalid_job_key() {
        let err = Parser::parse("job nocolon { disabled }").unwrap_err();
        assert!(matches!(err, ParseError::InvalidJobKey { .. }));
    }

    #[test]
    fn parse_full_croniqfile() {
        let src = r#"
# Croniqfile

import ./calendars.croniq

server {
  listen :9090
  data_dir /var/lib/croniq
  db sqlite
}

vars {
  default_tz Europe/Vienna
}

defaults {
  timezone {vars.default_tz}
  timeout 5m
}

calendar business-days {
  timezone "Europe/Vienna"
  include weekly monday tuesday wednesday thursday friday
  exclude annual 01-01 12-25 12-26
}

job billing:invoice {
  every weekday at 02:00 {
    calendar business-days
  }
  timeout 15m
  metadata { team billing; priority high }
}

job etl:sync {
  every 15 minutes
  timeout 10m
}
"#;
        let ast = Parser::parse(src).unwrap();
        // Count non-comment items
        let significant: Vec<_> = ast
            .items
            .iter()
            .filter(|i| !matches!(i, Item::Comment(_)))
            .collect();
        assert_eq!(significant.len(), 7); // import, server, vars, defaults, calendar, 2 jobs
    }

    #[test]
    fn parse_concurrency_group_block() {
        let ast = Parser::parse("concurrency_group api-x { max_concurrent 1 }").unwrap();
        let Item::ConcurrencyGroup(ref g) = ast.items[0] else {
            panic!("expected a concurrency_group item, got {:?}", ast.items[0]);
        };
        assert_eq!(g.name.value, "api-x");
        assert_eq!(g.directives.len(), 1);
        assert_eq!(g.directives[0].key.value, "max_concurrent");
        assert_eq!(g.directives[0].args[0].value, "1");
    }

    #[test]
    fn parse_concurrency_group_accepts_a_quoted_name() {
        let ast = Parser::parse(r#"concurrency_group "billing api" { max_concurrent 2 }"#).unwrap();
        let Item::ConcurrencyGroup(ref g) = ast.items[0] else {
            panic!("expected a concurrency_group item");
        };
        assert_eq!(g.name.value, "billing api");
    }

    #[test]
    fn parse_job_with_concurrency_group_directive() {
        let ast =
            Parser::parse("job sync:tickets { every day at 03:00\n concurrency_group api-x }")
                .unwrap();
        let Item::Job(ref job) = ast.items[0] else {
            panic!("expected a job");
        };
        let has_group = job.directives.iter().any(|dob| {
            matches!(dob, DirectiveOrBlock::Directive(d)
                if d.key.value == "concurrency_group" && d.args[0].value == "api-x")
        });
        assert!(has_group, "job must carry the concurrency_group directive");
    }
}
