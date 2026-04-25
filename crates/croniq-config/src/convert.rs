//! Cron expression → Croniq DSL converter.
//!
//! Translates standard 5-field cron expressions (`minute hour dom month dow`)
//! into Croniq's human-readable schedule syntax.
//!
//! ## Supported patterns
//!
//! | Cron expression    | Croniq equivalent              |
//! |--------------------|--------------------------------|
//! | `* * * * *`        | `every 1 minutes`              |
//! | `*/5 * * * *`      | `every 5 minutes`              |
//! | `*/30 * * * *`     | `every 30 minutes`             |
//! | `0 * * * *`        | `every 1 hours`                |
//! | `0 */2 * * *`      | `every 2 hours`                |
//! | `0 9 * * *`        | `every day at 09:00`           |
//! | `0 9 * * 1`        | `every monday at 09:00`        |
//! | `0 9 * * 1-5`      | `every weekday at 09:00`       |
//! | `0 9 * * 1,3,5`    | `every monday wednesday friday at 09:00` |
//! | `0 3 1 * *`        | `every 1st of month at 03:00`  |
//! | `0 3 1,15 * *`     | `every 1st 15th of month at 03:00` |
//! | `0 3 L * *`        | `every last of month at 03:00` |
//! | `@daily`           | `every day at 00:00`           |
//! | `@hourly`          | `every 1 hours`                |
//! | `@weekly`          | `every monday at 00:00`        |
//! | `@monthly`         | `every 1st of month at 00:00`  |
//! | `@yearly`/`@annually` | `every 1st of month at 00:00` (January — partial) |
//!
//! ## Limitations
//!
//! Cron expressions using both day-of-month AND day-of-week simultaneously
//! cannot be expressed in Croniq's DSL (which is intentional). The converter
//! will emit a best-effort translation with a warning in that case.

/// Conversion result: the Croniq schedule string plus optional warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionResult {
    /// The Croniq schedule expression (e.g. `every 5 minutes`).
    pub schedule: String,
    /// Human-readable warnings about patterns that couldn't be fully expressed.
    pub warnings: Vec<String>,
}

/// Convert a cron expression to a Croniq schedule string.
///
/// # Errors
///
/// Returns a human-readable error string if the expression cannot be parsed at all.
pub fn convert(expr: &str) -> Result<ConversionResult, String> {
    let expr = expr.trim();

    // Handle special @-macros first.
    match expr {
        "@yearly" | "@annually" => {
            return Ok(ConversionResult {
                schedule: "every 1st of month at 00:00".into(),
                warnings: vec![
                    "@yearly/@annually maps to 'every 1st of month' — \
                     Croniq does not have a yearly-only construct; \
                     add a `calendar` block to restrict to January if needed."
                        .into(),
                ],
            });
        }
        "@monthly" => {
            return Ok(ConversionResult {
                schedule: "every 1st of month at 00:00".into(),
                warnings: vec![],
            });
        }
        "@weekly" => {
            return Ok(ConversionResult {
                schedule: "every monday at 00:00".into(),
                warnings: vec![],
            });
        }
        "@daily" | "@midnight" => {
            return Ok(ConversionResult {
                schedule: "every day at 00:00".into(),
                warnings: vec![],
            });
        }
        "@hourly" => {
            return Ok(ConversionResult {
                schedule: "every 1 hours".into(),
                warnings: vec![],
            });
        }
        _ => {}
    }

    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(format!(
            "expected 5 fields (minute hour dom month dow), got {}",
            parts.len()
        ));
    }

    let (minute, hour, dom, _month, dow) = (parts[0], parts[1], parts[2], parts[3], parts[4]);

    let mut warnings = Vec::new();

    // ── Step-interval patterns (*/N) ──────────────────────────────────────────

    // `*/N * * * *` → every N minutes
    if is_step(minute) && is_star(hour) && is_star(dom) && is_star(dow) {
        let n = parse_step(minute)?;
        return Ok(ConversionResult {
            schedule: format!("every {n} minutes"),
            warnings,
        });
    }

    // `* * * * *` → every 1 minutes
    if is_star(minute) && is_star(hour) && is_star(dom) && is_star(dow) {
        return Ok(ConversionResult {
            schedule: "every 1 minutes".into(),
            warnings,
        });
    }

    // `0 */N * * *` → every N hours
    if is_zero(minute) && is_step(hour) && is_star(dom) && is_star(dow) {
        let n = parse_step(hour)?;
        return Ok(ConversionResult {
            schedule: format!("every {n} hours"),
            warnings,
        });
    }

    // `0 * * * *` → every 1 hours
    if is_zero(minute) && is_star(hour) && is_star(dom) && is_star(dow) {
        return Ok(ConversionResult {
            schedule: "every 1 hours".into(),
            warnings,
        });
    }

    // ── Time-of-day patterns (fixed minute + fixed hour) ──────────────────────

    let fixed_minute = parse_fixed(minute);
    let fixed_hour = parse_fixed(hour);

    if let (Some(m), Some(h)) = (fixed_minute, fixed_hour) {
        let time = format!("{h:02}:{m:02}");

        // dom and dow both wildcards → daily
        if is_star(dom) && is_star(dow) {
            return Ok(ConversionResult {
                schedule: format!("every day at {time}"),
                warnings,
            });
        }

        // dow specified, dom wildcard → weekday schedule
        if is_star(dom) && !is_star(dow) {
            let days = parse_dow(dow)?;
            let sched = format_weekday_schedule(&days, &time);
            return Ok(ConversionResult {
                schedule: sched,
                warnings,
            });
        }

        // dom specified (including "L"), dow wildcard → monthly schedule
        if !is_star(dom) && is_star(dow) {
            let ordinals = parse_dom(dom)?;
            let sched = format_monthly_schedule(&ordinals, &time);
            return Ok(ConversionResult {
                schedule: sched,
                warnings,
            });
        }

        // Both dom AND dow are set — ambiguous in Croniq
        if !is_star(dom) && !is_star(dow) {
            warnings.push(
                "Cron fires when EITHER dom OR dow matches; Croniq cannot express this \
                 directly. The converted schedule uses only the day-of-week field."
                    .into(),
            );
            let days = parse_dow(dow)?;
            let sched = format_weekday_schedule(&days, &time);
            return Ok(ConversionResult {
                schedule: sched,
                warnings,
            });
        }
    }

    // ── Fallback ──────────────────────────────────────────────────────────────

    Err(format!(
        "Could not convert '{expr}' to a Croniq schedule. \
         Supported patterns: */N (step intervals), fixed time-of-day \
         with weekday or dom constraints. Complex expressions with \
         ranges or lists in the time fields are not supported."
    ))
}

// ─── Field helpers ────────────────────────────────────────────────────────────

fn is_star(s: &str) -> bool {
    s == "*"
}

fn is_zero(s: &str) -> bool {
    s == "0"
}

fn is_step(s: &str) -> bool {
    s.starts_with("*/")
}

fn parse_step(s: &str) -> Result<u32, String> {
    s.trim_start_matches("*/")
        .parse::<u32>()
        .map_err(|_| format!("invalid step value in '{s}'"))
}

fn parse_fixed(s: &str) -> Option<u32> {
    s.parse::<u32>().ok()
}

// ─── Day-of-week parsing ──────────────────────────────────────────────────────

/// Day-of-week: numeric (0=Sun..6=Sat) or name.
fn parse_dow(s: &str) -> Result<Vec<String>, String> {
    // Range `1-5` → weekday, `0-6` or `*` → every day
    if s == "1-5" {
        return Ok(vec!["weekday".into()]);
    }
    if s == "0-6" || s == "6-0" {
        return Ok(vec![
            "sunday".into(),
            "monday".into(),
            "tuesday".into(),
            "wednesday".into(),
            "thursday".into(),
            "friday".into(),
            "saturday".into(),
        ]);
    }
    if s == "6,0" || s == "0,6" {
        return Ok(vec!["weekend".into()]);
    }
    if s == "6-7" || s == "0,7" {
        return Ok(vec!["weekend".into()]);
    }

    // Comma-list or single value
    let items: Vec<&str> = s.split(',').collect();
    let mut days = Vec::new();
    for item in &items {
        days.push(dow_to_name(item)?);
    }
    Ok(days)
}

fn dow_to_name(s: &str) -> Result<String, String> {
    let name = match s.to_lowercase().as_str() {
        "0" | "7" | "sun" | "sunday" => "sunday",
        "1" | "mon" | "monday" => "monday",
        "2" | "tue" | "tuesday" => "tuesday",
        "3" | "wed" | "wednesday" => "wednesday",
        "4" | "thu" | "thursday" => "thursday",
        "5" | "fri" | "friday" => "friday",
        "6" | "sat" | "saturday" => "saturday",
        other => return Err(format!("unknown day-of-week value '{other}'")),
    };
    Ok(name.into())
}

fn format_weekday_schedule(days: &[String], time: &str) -> String {
    // Single-item shorthands
    if days == ["weekday"] {
        return format!("every weekday at {time}");
    }
    if days == ["weekend"] {
        return format!("every weekend at {time}");
    }
    if days.len() == 7 {
        return format!("every day at {time}");
    }
    if days.len() == 1 {
        return format!("every {} at {time}", days[0]);
    }
    format!("every {} at {time}", days.join(" "))
}

// ─── Day-of-month parsing ─────────────────────────────────────────────────────

fn parse_dom(s: &str) -> Result<Vec<String>, String> {
    // "L" = last day of month
    if s.eq_ignore_ascii_case("L") {
        return Ok(vec!["last".into()]);
    }

    let items: Vec<&str> = s.split(',').collect();
    let mut ordinals = Vec::new();
    for item in &items {
        let n: u32 = item
            .parse()
            .map_err(|_| format!("invalid day-of-month value '{item}'"))?;
        if n == 0 || n > 31 {
            return Err(format!("day-of-month {n} out of range (1–31)"));
        }
        ordinals.push(format!("{}{}", n, ordinal_suffix(n)));
    }
    Ok(ordinals)
}

fn ordinal_suffix(n: u32) -> &'static str {
    match n % 100 {
        11..=13 => "th",
        _ => match n % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        },
    }
}

fn format_monthly_schedule(ordinals: &[String], time: &str) -> String {
    format!("every {} of month at {time}", ordinals.join(" "))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(expr: &str) -> String {
        convert(expr).unwrap().schedule
    }

    fn conv_warn(expr: &str) -> (String, Vec<String>) {
        let r = convert(expr).unwrap();
        (r.schedule, r.warnings)
    }

    // ── Step intervals ────────────────────────────────────────────────────────

    #[test]
    fn every_minute() {
        assert_eq!(conv("* * * * *"), "every 1 minutes");
    }

    #[test]
    fn every_5_minutes() {
        assert_eq!(conv("*/5 * * * *"), "every 5 minutes");
    }

    #[test]
    fn every_15_minutes() {
        assert_eq!(conv("*/15 * * * *"), "every 15 minutes");
    }

    #[test]
    fn every_30_minutes() {
        assert_eq!(conv("*/30 * * * *"), "every 30 minutes");
    }

    #[test]
    fn every_hour() {
        assert_eq!(conv("0 * * * *"), "every 1 hours");
    }

    #[test]
    fn every_2_hours() {
        assert_eq!(conv("0 */2 * * *"), "every 2 hours");
    }

    #[test]
    fn every_6_hours() {
        assert_eq!(conv("0 */6 * * *"), "every 6 hours");
    }

    // ── Daily ─────────────────────────────────────────────────────────────────

    #[test]
    fn daily_at_midnight() {
        assert_eq!(conv("0 0 * * *"), "every day at 00:00");
    }

    #[test]
    fn daily_at_9am() {
        assert_eq!(conv("0 9 * * *"), "every day at 09:00");
    }

    #[test]
    fn daily_at_2am_30min() {
        assert_eq!(conv("30 2 * * *"), "every day at 02:30");
    }

    // ── Weekday patterns ──────────────────────────────────────────────────────

    #[test]
    fn every_monday() {
        assert_eq!(conv("0 9 * * 1"), "every monday at 09:00");
    }

    #[test]
    fn every_friday() {
        assert_eq!(conv("0 17 * * 5"), "every friday at 17:00");
    }

    #[test]
    fn weekdays() {
        assert_eq!(conv("0 9 * * 1-5"), "every weekday at 09:00");
    }

    #[test]
    fn mon_wed_fri() {
        assert_eq!(
            conv("0 9 * * 1,3,5"),
            "every monday wednesday friday at 09:00"
        );
    }

    #[test]
    fn every_weekend() {
        assert_eq!(conv("0 10 * * 6,0"), "every weekend at 10:00");
    }

    #[test]
    fn dow_by_name() {
        assert_eq!(conv("0 8 * * Mon"), "every monday at 08:00");
    }

    // ── Monthly patterns ──────────────────────────────────────────────────────

    #[test]
    fn first_of_month() {
        assert_eq!(conv("0 3 1 * *"), "every 1st of month at 03:00");
    }

    #[test]
    fn fifteenth_of_month() {
        assert_eq!(conv("0 6 15 * *"), "every 15th of month at 06:00");
    }

    #[test]
    fn first_and_fifteenth() {
        assert_eq!(conv("0 10 1,15 * *"), "every 1st 15th of month at 10:00");
    }

    #[test]
    fn last_of_month() {
        assert_eq!(conv("0 23 L * *"), "every last of month at 23:00");
    }

    #[test]
    fn eleventh_of_month_has_th_suffix() {
        assert_eq!(conv("0 0 11 * *"), "every 11th of month at 00:00");
    }

    #[test]
    fn twenty_second_has_nd_suffix() {
        assert_eq!(conv("0 0 22 * *"), "every 22nd of month at 00:00");
    }

    // ── @-macros ──────────────────────────────────────────────────────────────

    #[test]
    fn at_daily() {
        assert_eq!(conv("@daily"), "every day at 00:00");
    }

    #[test]
    fn at_midnight() {
        assert_eq!(conv("@midnight"), "every day at 00:00");
    }

    #[test]
    fn at_hourly() {
        assert_eq!(conv("@hourly"), "every 1 hours");
    }

    #[test]
    fn at_weekly() {
        assert_eq!(conv("@weekly"), "every monday at 00:00");
    }

    #[test]
    fn at_monthly() {
        assert_eq!(conv("@monthly"), "every 1st of month at 00:00");
    }

    #[test]
    fn at_yearly_emits_warning() {
        let (sched, warnings) = conv_warn("@yearly");
        assert_eq!(sched, "every 1st of month at 00:00");
        assert!(!warnings.is_empty());
        assert!(warnings[0].contains("@yearly"));
    }

    // ── Warnings ──────────────────────────────────────────────────────────────

    #[test]
    fn dom_and_dow_emits_warning() {
        let (sched, warnings) = conv_warn("0 9 1 * 1");
        // Uses dow (monday) as primary, warns about ambiguity
        assert_eq!(sched, "every monday at 09:00");
        assert!(!warnings.is_empty());
        assert!(warnings[0].contains("dom OR dow"));
    }

    // ── Errors ────────────────────────────────────────────────────────────────

    #[test]
    fn wrong_field_count_is_error() {
        assert!(convert("* * * *").is_err());
        assert!(convert("* * * * * *").is_err());
    }

    #[test]
    fn unknown_at_macro_is_error() {
        assert!(convert("@reboot").is_err());
    }

    #[test]
    fn invalid_dow_is_error() {
        assert!(convert("0 9 * * Funday").is_err());
    }
}
