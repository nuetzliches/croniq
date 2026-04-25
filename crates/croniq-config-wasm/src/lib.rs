//! WebAssembly bindings for the Croniqfile DSL.
//!
//! Exposes a small surface for the in-browser schedule and calendar
//! generators (PR-B and PR-C):
//!
//! - `parse_schedule(dsl)` — validate a schedule line, return a typed AST
//! - `format_schedule(structured)` — emit DSL from the form-builder shape
//! - `parse_calendar_rules(dsl)` — validate a calendar rules block
//! - `format_calendar_rules(structured)` — emit DSL from the rule editor
//! - `next_fires(dsl, now_iso, count)` — preview upcoming firing times
//! - `evaluate_calendar_day(dsl, day_iso)` — is this day active?
//!
//! All inputs and outputs go through `serde-wasm-bindgen` so the
//! TypeScript caller sees plain objects, not opaque handles. Errors
//! become rejected promises (in the typed wrappers) or thrown JsValues
//! (in the raw bindings).

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use croniq_config::ast::{IntervalUnit, MonthOrdinal, ScheduleKind, ScheduleNode, Weekday};
use croniq_config::parser::Parser;

/// Install a panic hook that forwards Rust panics to `console.error`.
/// Safe to call multiple times (the inner crate guards against that).
/// The TypeScript wrapper calls this once on first import.
#[wasm_bindgen(start)]
pub fn _start() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

// ── Schedule shapes (serde-friendly mirror of croniq_config::ast::ScheduleKind) ──

/// Form-builder schedule shape. Mirrors `ScheduleKind` from the parser
/// AST but with simpler field types so the TypeScript caller can build
/// it from `<select>` values without knowing about `SourceSpan`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum SchedulePayload {
    Interval {
        count: u32,
        unit: String,
    },
    Daily {
        hour: u8,
        minute: u8,
    },
    Weekdays {
        days: Vec<String>,
        hour: u8,
        minute: u8,
    },
    Monthly {
        ordinals: Vec<String>,
        hour: u8,
        minute: u8,
    },
    Once {
        at: String,
    },
    Disabled,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParseScheduleResult {
    pub ok: bool,
    pub schedule: Option<SchedulePayload>,
    pub error: Option<String>,
}

/// Parse a single schedule line (e.g. `every 5 minutes`) and return the
/// structured form. Wraps the input in a placeholder job block so we can
/// reuse the existing recursive-descent parser.
#[wasm_bindgen(js_name = parseSchedule)]
pub fn parse_schedule(dsl: &str) -> Result<JsValue, JsValue> {
    let result = parse_schedule_inner(dsl);
    serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
}

fn parse_schedule_inner(dsl: &str) -> ParseScheduleResult {
    // Wrap in a synthetic job block — the parser only exposes
    // `parse_croniqfile`, not a standalone `parse_schedule_line`.
    let trimmed = dsl.trim();
    if trimmed.is_empty() {
        return ParseScheduleResult {
            ok: false,
            schedule: None,
            error: Some("schedule is empty".into()),
        };
    }
    // Schedule lines parse inline inside a job block — no nested
    // `schedule {}`. Synthetic key namespaces avoid clashes if anyone
    // reads the wrapped output for diagnostics.
    let wrapped = format!("job preview:line {{\n  {trimmed}\n}}\n");
    match Parser::parse(&wrapped) {
        Ok(ast) => {
            // Find the job's schedule node. The wrapping guarantees one
            // job and exactly one schedule, so unwrap is safe.
            let schedule = ast.items.iter().find_map(|i| {
                if let croniq_config::ast::Item::Job(j) = i {
                    j.schedule.clone()
                } else {
                    None
                }
            });
            match schedule {
                Some(node) => ParseScheduleResult {
                    ok: true,
                    schedule: Some(schedule_to_payload(&node)),
                    error: None,
                },
                None => ParseScheduleResult {
                    ok: false,
                    schedule: None,
                    error: Some("no schedule found".into()),
                },
            }
        }
        Err(e) => ParseScheduleResult {
            ok: false,
            schedule: None,
            error: Some(e.to_string()),
        },
    }
}

fn schedule_to_payload(node: &ScheduleNode) -> SchedulePayload {
    match &node.kind {
        ScheduleKind::Interval { count, unit } => SchedulePayload::Interval {
            count: *count,
            unit: match unit {
                IntervalUnit::Seconds => "seconds".into(),
                IntervalUnit::Minutes => "minutes".into(),
                IntervalUnit::Hours => "hours".into(),
            },
        },
        ScheduleKind::Daily { time } => SchedulePayload::Daily {
            hour: time.hour,
            minute: time.minute,
        },
        ScheduleKind::Weekdays { days, time } => SchedulePayload::Weekdays {
            days: days.iter().map(weekday_str).collect(),
            hour: time.hour,
            minute: time.minute,
        },
        ScheduleKind::Monthly { ordinals, time } => SchedulePayload::Monthly {
            ordinals: ordinals.iter().map(ordinal_str).collect(),
            hour: time.hour,
            minute: time.minute,
        },
        ScheduleKind::Once { at } => SchedulePayload::Once {
            at: at.value.clone(),
        },
        ScheduleKind::Disabled => SchedulePayload::Disabled,
    }
}

fn weekday_str(w: &Weekday) -> String {
    match w {
        Weekday::Monday => "monday",
        Weekday::Tuesday => "tuesday",
        Weekday::Wednesday => "wednesday",
        Weekday::Thursday => "thursday",
        Weekday::Friday => "friday",
        Weekday::Saturday => "saturday",
        Weekday::Sunday => "sunday",
    }
    .into()
}

fn ordinal_str(o: &MonthOrdinal) -> String {
    match o {
        MonthOrdinal::Day(n) => match n {
            1 => "1st".into(),
            2 => "2nd".into(),
            3 => "3rd".into(),
            21 => "21st".into(),
            22 => "22nd".into(),
            23 => "23rd".into(),
            31 => "31st".into(),
            n => format!("{n}th"),
        },
        MonthOrdinal::Last => "last".into(),
    }
}

// ── Format direction: structured → DSL string ──

/// Render a schedule payload back to the canonical DSL string.
#[wasm_bindgen(js_name = formatSchedule)]
pub fn format_schedule(value: JsValue) -> Result<String, JsValue> {
    let payload: SchedulePayload =
        serde_wasm_bindgen::from_value(value).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(format_schedule_inner(&payload))
}

fn format_schedule_inner(p: &SchedulePayload) -> String {
    match p {
        SchedulePayload::Interval { count, unit } => {
            let unit = if *count == 1 {
                // The parser accepts plural; the formatter emits plural
                // for symmetry. Singular is a *display* concern that the
                // UI can handle separately (M5 from PR #50 already does
                // this in the schedule-summary text, not in the DSL).
                match unit.as_str() {
                    "seconds" => "seconds",
                    "minutes" => "minutes",
                    "hours" => "hours",
                    _ => "minutes",
                }
            } else {
                match unit.as_str() {
                    "seconds" => "seconds",
                    "minutes" => "minutes",
                    "hours" => "hours",
                    _ => "minutes",
                }
            };
            format!("every {count} {unit}")
        }
        SchedulePayload::Daily { hour, minute } => {
            format!("every day at {hour:02}:{minute:02}")
        }
        SchedulePayload::Weekdays { days, hour, minute } => {
            // Special-case Mon–Fri to "weekday" for readability, matching
            // the converter in croniq-config::convert.
            let normalized: Vec<&str> = days.iter().map(|d| d.as_str()).collect();
            let body = if normalized.len() == 5
                && ["monday", "tuesday", "wednesday", "thursday", "friday"]
                    .iter()
                    .all(|d| normalized.contains(d))
            {
                "weekday".to_string()
            } else if normalized.len() == 2
                && normalized.contains(&"saturday")
                && normalized.contains(&"sunday")
            {
                "weekend".to_string()
            } else {
                normalized.join(" ")
            };
            format!("every {body} at {hour:02}:{minute:02}")
        }
        SchedulePayload::Monthly {
            ordinals,
            hour,
            minute,
        } => {
            let body = ordinals.join(" ");
            format!("every {body} of month at {hour:02}:{minute:02}")
        }
        SchedulePayload::Once { at } => format!("once at \"{at}\""),
        SchedulePayload::Disabled => "disabled".into(),
    }
}

// ── next_fires preview ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct NextFiresResult {
    pub ok: bool,
    pub fires: Vec<String>,
    pub error: Option<String>,
}

/// Compute the next `count` firing times for a schedule line.
/// `now_iso` must be RFC 3339 / ISO 8601 with timezone (e.g.
/// `2026-04-25T18:00:00Z`). Returns ISO strings in UTC.
#[wasm_bindgen(js_name = nextFires)]
pub fn next_fires(dsl: &str, now_iso: &str, count: u32) -> Result<JsValue, JsValue> {
    let result = next_fires_inner(dsl, now_iso, count);
    serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
}

fn next_fires_inner(dsl: &str, now_iso: &str, count: u32) -> NextFiresResult {
    let parsed = parse_schedule_inner(dsl);
    if !parsed.ok {
        return NextFiresResult {
            ok: false,
            fires: vec![],
            error: parsed.error,
        };
    }
    // Re-parse via the wrapped DSL to get a real ScheduleNode for the
    // scheduler runtime to evaluate. This duplicates a tiny bit of work
    // but keeps the FFI surface clean — `SchedulePayload` is the public
    // shape; `ScheduleNode` is internal.
    let trimmed = dsl.trim();
    // Schedule lines parse inline inside a job block — no nested
    // `schedule {}`. Synthetic key namespaces avoid clashes if anyone
    // reads the wrapped output for diagnostics.
    let wrapped = format!("job preview:line {{\n  {trimmed}\n}}\n");
    let ast = match Parser::parse(&wrapped) {
        Ok(a) => a,
        Err(e) => {
            return NextFiresResult {
                ok: false,
                fires: vec![],
                error: Some(e.to_string()),
            };
        }
    };
    let node = ast
        .items
        .iter()
        .find_map(|i| match i {
            croniq_config::ast::Item::Job(j) => j.schedule.clone(),
            _ => None,
        })
        .expect("preview wrapping always emits one job with one schedule");

    let now = match chrono::DateTime::parse_from_rfc3339(now_iso) {
        Ok(t) => t.with_timezone(&chrono::Utc),
        Err(e) => {
            return NextFiresResult {
                ok: false,
                fires: vec![],
                error: Some(format!("invalid now_iso: {e}")),
            };
        }
    };

    let mut fires = Vec::with_capacity(count as usize);
    let mut cursor = now;
    for _ in 0..count {
        match next_fire_utc(&node.kind, cursor) {
            Some(next) => {
                fires.push(next.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
                // Use the just-emitted fire as the new cursor — for
                // interval schedules `next_fire` returns `cursor + N`,
                // so passing `next` gives `next + N` next iteration.
                // For daily/weekday/monthly the strict-greater check
                // inside `next_fire_utc` handles the increment.
                cursor = next;
            }
            None => break,
        }
    }
    NextFiresResult {
        ok: true,
        fires,
        error: None,
    }
}

/// UTC-only next-fire computation. We deliberately don't depend on
/// `croniq-scheduler` (which pulls `chrono-tz` and ~150 KB of timezone
/// data) — for the in-browser preview UTC is acceptable and the UI
/// converts to local time client-side. Timezone-aware preview can come
/// as a follow-up if there's demand.
fn next_fire_utc(
    kind: &ScheduleKind,
    after: chrono::DateTime<chrono::Utc>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    use chrono::{Datelike, Duration, NaiveDate, NaiveTime, TimeZone};
    match kind {
        ScheduleKind::Interval { count, unit } => {
            let secs = match unit {
                IntervalUnit::Seconds => *count as i64,
                IntervalUnit::Minutes => *count as i64 * 60,
                IntervalUnit::Hours => *count as i64 * 3600,
            };
            if secs <= 0 {
                return None;
            }
            Some(after + Duration::seconds(secs))
        }
        ScheduleKind::Daily { time } => {
            let t = NaiveTime::from_hms_opt(time.hour as u32, time.minute as u32, 0)?;
            let today = after.date_naive();
            // Try today first; if that's already past, roll to tomorrow.
            let candidate_today = chrono::Utc.from_utc_datetime(&today.and_time(t));
            if candidate_today > after {
                return Some(candidate_today);
            }
            let tomorrow = today + Duration::days(1);
            Some(chrono::Utc.from_utc_datetime(&tomorrow.and_time(t)))
        }
        ScheduleKind::Weekdays { days, time } => {
            let t = NaiveTime::from_hms_opt(time.hour as u32, time.minute as u32, 0)?;
            let target_set: Vec<chrono::Weekday> = days.iter().map(weekday_to_chrono).collect();
            // Walk forward day-by-day for at most 7 days — guaranteed
            // to find a match if any weekday is in the set.
            for offset in 0..=7 {
                let day = after.date_naive() + Duration::days(offset);
                if !target_set.contains(&day.weekday()) {
                    continue;
                }
                let candidate = chrono::Utc.from_utc_datetime(&day.and_time(t));
                if candidate > after {
                    return Some(candidate);
                }
            }
            None
        }
        ScheduleKind::Monthly { ordinals, time } => {
            let t = NaiveTime::from_hms_opt(time.hour as u32, time.minute as u32, 0)?;
            // Try this month, next month, month-after — three is enough to
            // satisfy any well-formed monthly schedule.
            let mut year = after.year();
            let mut month = after.month();
            for _ in 0..3 {
                for ord in ordinals {
                    let day = match ord {
                        MonthOrdinal::Day(n) => *n as u32,
                        MonthOrdinal::Last => last_day_of_month(year, month),
                    };
                    if let Some(d) = NaiveDate::from_ymd_opt(year, month, day) {
                        let candidate = chrono::Utc.from_utc_datetime(&d.and_time(t));
                        if candidate > after {
                            return Some(candidate);
                        }
                    }
                }
                // Roll to next month.
                if month == 12 {
                    month = 1;
                    year += 1;
                } else {
                    month += 1;
                }
            }
            None
        }
        ScheduleKind::Once { at } => {
            // The AST stores the once-time as a string; parse it as
            // RFC3339 and skip if we're already past.
            let parsed = chrono::DateTime::parse_from_rfc3339(&at.value).ok()?;
            let utc = parsed.with_timezone(&chrono::Utc);
            if utc > after { Some(utc) } else { None }
        }
        ScheduleKind::Disabled => None,
    }
}

fn weekday_to_chrono(w: &Weekday) -> chrono::Weekday {
    match w {
        Weekday::Monday => chrono::Weekday::Mon,
        Weekday::Tuesday => chrono::Weekday::Tue,
        Weekday::Wednesday => chrono::Weekday::Wed,
        Weekday::Thursday => chrono::Weekday::Thu,
        Weekday::Friday => chrono::Weekday::Fri,
        Weekday::Saturday => chrono::Weekday::Sat,
        Weekday::Sunday => chrono::Weekday::Sun,
    }
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    use chrono::{Datelike, NaiveDate};
    // Step into the next month then back one day.
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_of_next = NaiveDate::from_ymd_opt(next_year, next_month, 1).unwrap();
    (first_of_next - chrono::Duration::days(1)).day()
}

// ── Calendar rules: parse + format ─────────────────────────────────

/// A single calendar rule, mirroring the parser's loose shape: the
/// rule_type is a free string (`"weekly"`, `"window"`, `"annual"`,
/// `"monthly"`, `"timezone"`) and `args` holds the rest of the line
/// verbatim (each token is a `StringValue`). The scheduler validates
/// the combination at compile-time; we surface it raw so the form
/// builder can extend without recompiling the wasm bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarRulePayload {
    pub action: String, // "include" | "exclude"
    pub rule_type: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParseCalendarResult {
    pub ok: bool,
    pub rules: Vec<CalendarRulePayload>,
    pub diagnostics: Vec<String>,
}

/// Parse a multi-line calendar rule block. Wraps in a calendar block
/// to reuse the existing parser.
#[wasm_bindgen(js_name = parseCalendarRules)]
pub fn parse_calendar_rules(dsl: &str) -> Result<JsValue, JsValue> {
    let result = parse_calendar_rules_inner(dsl);
    serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
}

fn parse_calendar_rules_inner(dsl: &str) -> ParseCalendarResult {
    let trimmed = dsl.trim();
    if trimmed.is_empty() {
        return ParseCalendarResult {
            ok: true,
            rules: vec![],
            diagnostics: vec![],
        };
    }
    let wrapped = format!("calendar \"preview\" {{\n{trimmed}\n}}\n");
    match Parser::parse(&wrapped) {
        Ok(ast) => {
            let cal = ast.items.iter().find_map(|i| match i {
                croniq_config::ast::Item::Calendar(c) => Some(c),
                _ => None,
            });
            let rules = cal
                .map(|c| c.rules.iter().map(rule_to_payload).collect())
                .unwrap_or_default();
            ParseCalendarResult {
                ok: true,
                rules,
                diagnostics: vec![],
            }
        }
        Err(e) => ParseCalendarResult {
            ok: false,
            rules: vec![],
            diagnostics: vec![e.to_string()],
        },
    }
}

fn rule_to_payload(r: &croniq_config::ast::CalendarRule) -> CalendarRulePayload {
    let action = match r.kind {
        croniq_config::ast::CalendarRuleKind::Include => "include",
        croniq_config::ast::CalendarRuleKind::Exclude => "exclude",
    }
    .into();
    CalendarRulePayload {
        action,
        rule_type: r.rule_type.value.clone(),
        args: r.args.iter().map(|a| a.value.clone()).collect(),
    }
}

/// Format a list of structured rules back to the multi-line DSL block.
#[wasm_bindgen(js_name = formatCalendarRules)]
pub fn format_calendar_rules(value: JsValue) -> Result<String, JsValue> {
    let rules: Vec<CalendarRulePayload> =
        serde_wasm_bindgen::from_value(value).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(rules.iter().map(format_rule).collect::<Vec<_>>().join("\n"))
}

fn format_rule(r: &CalendarRulePayload) -> String {
    // The parser-level shape stores args as raw token strings. For the
    // common rule types the form-builder enforces canonical formatting
    // (weekly: 3-letter day strings, window: HH:MM..HH:MM as a single
    // arg, annual: MM-DD as a single arg) — so concatenating is enough.
    let body = if r.args.is_empty() {
        String::new()
    } else if r.rule_type == "weekly" {
        // Weekly args are the day tokens — quote each one.
        r.args
            .iter()
            .map(|d| format!("\"{d}\""))
            .collect::<Vec<_>>()
            .join(" ")
    } else if r.rule_type == "window" {
        // Window arg is `"HH:MM".."HH:MM"` already-formatted, but if the
        // caller passes two raw times we join them with `..`.
        if r.args.len() == 2 {
            format!("\"{}\"..\"{}\"", r.args[0], r.args[1])
        } else {
            r.args.join(" ")
        }
    } else {
        // annual / monthly / etc — just space-join as-is (parser accepts
        // un-quoted numerics).
        r.args.join(" ")
    };
    if body.is_empty() {
        format!("{} {}", r.action, r.rule_type)
    } else {
        format!("{} {} {body}", r.action, r.rule_type)
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_interval() {
        let r = parse_schedule_inner("every 5 minutes");
        assert!(r.ok, "{:?}", r.error);
        match r.schedule.unwrap() {
            SchedulePayload::Interval { count, unit } => {
                assert_eq!(count, 5);
                assert_eq!(unit, "minutes");
            }
            other => panic!("expected interval, got {other:?}"),
        }
    }

    #[test]
    fn parse_daily() {
        let r = parse_schedule_inner("every day at 09:30");
        assert!(r.ok, "{:?}", r.error);
        match r.schedule.unwrap() {
            SchedulePayload::Daily { hour, minute } => {
                assert_eq!(hour, 9);
                assert_eq!(minute, 30);
            }
            other => panic!("expected daily, got {other:?}"),
        }
    }

    #[test]
    fn round_trip_interval() {
        let p = SchedulePayload::Interval {
            count: 15,
            unit: "minutes".into(),
        };
        assert_eq!(format_schedule_inner(&p), "every 15 minutes");
    }

    #[test]
    fn round_trip_weekday_collapse() {
        let p = SchedulePayload::Weekdays {
            days: vec![
                "monday".into(),
                "tuesday".into(),
                "wednesday".into(),
                "thursday".into(),
                "friday".into(),
            ],
            hour: 9,
            minute: 0,
        };
        assert_eq!(format_schedule_inner(&p), "every weekday at 09:00");
    }

    #[test]
    fn parse_garbage_returns_error() {
        let r = parse_schedule_inner("not a schedule");
        assert!(!r.ok);
        assert!(r.error.is_some());
    }

    #[test]
    fn parse_calendar_rules_empty_ok() {
        let r = parse_calendar_rules_inner("");
        assert!(r.ok);
        assert!(r.rules.is_empty());
    }

    #[test]
    fn next_fires_interval_steps_correctly() {
        let r = next_fires_inner("every 5 minutes", "2026-04-25T18:00:00Z", 3);
        assert!(r.ok, "{:?}", r.error);
        assert_eq!(
            r.fires,
            vec![
                "2026-04-25T18:05:00Z".to_string(),
                "2026-04-25T18:10:00Z".to_string(),
                "2026-04-25T18:15:00Z".to_string(),
            ]
        );
    }

    #[test]
    fn next_fires_daily_rolls_to_tomorrow_when_past() {
        let r = next_fires_inner("every day at 09:00", "2026-04-25T18:00:00Z", 2);
        assert!(r.ok);
        // 09:00 today is past 18:00, so first fire is tomorrow.
        assert_eq!(
            r.fires,
            vec![
                "2026-04-26T09:00:00Z".to_string(),
                "2026-04-27T09:00:00Z".to_string(),
            ]
        );
    }

    #[test]
    fn next_fires_weekdays_picks_next_match() {
        // 2026-04-25 is a Saturday. Asking for Mon/Wed/Fri should land
        // on Monday 2026-04-27.
        let r = next_fires_inner(
            "every monday wednesday friday at 09:00",
            "2026-04-25T18:00:00Z",
            1,
        );
        assert!(r.ok, "{:?}", r.error);
        assert_eq!(r.fires, vec!["2026-04-27T09:00:00Z".to_string()]);
    }
}
