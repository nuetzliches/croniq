use crate::lexer::Span;
use serde::Serialize;

/// A complete Croniqfile AST.
#[derive(Debug, Clone, Serialize)]
pub struct Croniqfile {
    pub items: Vec<Item>,
    pub span: Span,
}

/// Top-level item in a Croniqfile.
#[derive(Debug, Clone, Serialize)]
pub enum Item {
    Import(Import),
    Server(ServerBlock),
    PullApi(PullApiBlock),
    Observability(ObservabilityBlock),
    Vars(VarsBlock),
    Defaults(DefaultsBlock),
    Calendar(CalendarBlock),
    Job(JobBlock),
    Comment(CommentNode),
}

// ─── Comments ───

#[derive(Debug, Clone, Serialize)]
pub struct CommentNode {
    pub text: String,
    pub span: Span,
}

// ─── Import ───

#[derive(Debug, Clone, Serialize)]
pub struct Import {
    pub path: StringValue,
    pub span: Span,
}

// ─── Server ───

#[derive(Debug, Clone, Serialize)]
pub struct ServerBlock {
    pub directives: Vec<Directive>,
    pub span: Span,
}

// ─── Pull API ───

#[derive(Debug, Clone, Serialize)]
pub struct PullApiBlock {
    pub directives: Vec<Directive>,
    pub span: Span,
}

// ─── Observability ───

#[derive(Debug, Clone, Serialize)]
pub struct ObservabilityBlock {
    pub sub_blocks: Vec<NamedBlock>,
    pub span: Span,
}

// ─── Vars ───

#[derive(Debug, Clone, Serialize)]
pub struct VarsBlock {
    pub entries: Vec<Directive>,
    pub span: Span,
}

// ─── Defaults ───

#[derive(Debug, Clone, Serialize)]
pub struct DefaultsBlock {
    pub directives: Vec<DirectiveOrBlock>,
    pub span: Span,
}

// ─── Calendar ───

#[derive(Debug, Clone, Serialize)]
pub struct CalendarBlock {
    pub name: StringValue,
    pub rules: Vec<CalendarRule>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct CalendarRule {
    pub kind: CalendarRuleKind,
    pub rule_type: StringValue,
    pub args: Vec<StringValue>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CalendarRuleKind {
    Include,
    Exclude,
}

// ─── Job ───

#[derive(Debug, Clone, Serialize)]
pub struct JobBlock {
    pub key: JobKey,
    pub schedule: Option<ScheduleNode>,
    pub directives: Vec<DirectiveOrBlock>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobKey {
    pub raw: String,
    pub namespace: String,
    pub name: String,
    pub variant: Option<String>,
    pub span: Span,
}

// ─── Schedule ───

/// Optional execution-mode prefix on a schedule line.
///
/// ```text
/// ephemeral every 1 seconds     # fire-and-forget, no persistence
/// queued    every day at 02:00   # guaranteed delivery (default)
///           every 15 minutes     # no prefix → queued
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ScheduleMode {
    Queued,
    Ephemeral,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScheduleNode {
    pub kind: ScheduleKind,
    /// Explicit execution-mode prefix (`ephemeral` / `queued`), if present.
    pub mode: Option<ScheduleMode>,
    pub options: Vec<Directive>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub enum ScheduleKind {
    /// `every N seconds/minutes/hours`
    Interval { count: u32, unit: IntervalUnit },

    /// `every day at HH:MM`
    Daily { time: TimeValue },

    /// `every monday [tuesday ...] at HH:MM`
    /// `every weekday at HH:MM`
    /// `every weekend at HH:MM`
    Weekdays { days: Vec<Weekday>, time: TimeValue },

    /// `every 1st [15th ...] of month at HH:MM`
    Monthly {
        ordinals: Vec<MonthOrdinal>,
        time: TimeValue,
    },

    /// `once at <datetime>`
    Once { at: StringValue },

    /// `disabled`
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum IntervalUnit {
    Seconds,
    Minutes,
    Hours,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimeValue {
    pub hour: u8,
    pub minute: u8,
    pub raw: String,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "monday" => Some(Self::Monday),
            "tuesday" => Some(Self::Tuesday),
            "wednesday" => Some(Self::Wednesday),
            "thursday" => Some(Self::Thursday),
            "friday" => Some(Self::Friday),
            "saturday" => Some(Self::Saturday),
            "sunday" => Some(Self::Sunday),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Monday => "monday",
            Self::Tuesday => "tuesday",
            Self::Wednesday => "wednesday",
            Self::Thursday => "thursday",
            Self::Friday => "friday",
            Self::Saturday => "saturday",
            Self::Sunday => "sunday",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub enum MonthOrdinal {
    /// 1st, 2nd, 3rd, ... 31st
    Day(u8),
    /// Last day of month
    Last,
}

// ─── Shared types ───

/// A value that is either an unquoted ident, a quoted string, or a placeholder.
#[derive(Debug, Clone, Serialize)]
pub struct StringValue {
    pub value: String,
    pub quoted: bool,
    pub is_placeholder: bool,
    pub span: Span,
}

/// A simple directive: `key value1 value2 ...`
#[derive(Debug, Clone, Serialize)]
pub struct Directive {
    pub key: StringValue,
    pub args: Vec<StringValue>,
    pub span: Span,
}

/// Either a directive or a named sub-block.
#[derive(Debug, Clone, Serialize)]
pub enum DirectiveOrBlock {
    Directive(Directive),
    Block(NamedBlock),
    Comment(CommentNode),
}

/// A named block: `name [qualifier] { ... }`
#[derive(Debug, Clone, Serialize)]
pub struct NamedBlock {
    pub name: StringValue,
    pub qualifier: Option<StringValue>,
    pub directives: Vec<DirectiveOrBlock>,
    pub span: Span,
}
