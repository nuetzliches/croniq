//! Round-trip safe formatter for Croniqfile AST.
//! Preserves comments, blank lines, and quoting style.

use crate::ast::*;

/// Format a Croniqfile AST back to source text.
pub fn format(ast: &Croniqfile) -> String {
    let mut out = String::new();
    let mut first = true;

    for item in &ast.items {
        if !first && !matches!(item, Item::Comment(_)) {
            out.push('\n');
        }
        first = false;
        format_item(&mut out, item, 0);
    }

    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn format_item(out: &mut String, item: &Item, indent: usize) {
    match item {
        Item::Comment(c) => {
            write_indent(out, indent);
            out.push_str(&format!("# {}\n", c.text));
        }
        Item::Import(i) => {
            out.push_str(&format!("import {}\n", format_string_value(&i.path)));
        }
        Item::Server(s) => {
            out.push_str("server {\n");
            format_directives(out, &s.directives, indent + 1);
            out.push_str("}\n");
        }
        Item::PullApi(p) => {
            out.push_str("pull_api {\n");
            format_directives(out, &p.directives, indent + 1);
            out.push_str("}\n");
        }
        Item::Observability(o) => {
            out.push_str("observability {\n");
            for block in &o.sub_blocks {
                format_named_block(out, block, indent + 1);
            }
            out.push_str("}\n");
        }
        Item::Vars(v) => {
            out.push_str("vars {\n");
            format_directives(out, &v.entries, indent + 1);
            out.push_str("}\n");
        }
        Item::Defaults(d) => {
            out.push_str("defaults {\n");
            format_directives_or_blocks(out, &d.directives, indent + 1);
            out.push_str("}\n");
        }
        Item::Calendar(c) => {
            out.push_str(&format!("calendar {} {{\n", format_string_value(&c.name)));
            for rule in &c.rules {
                format_calendar_rule(out, rule, indent + 1);
            }
            out.push_str("}\n");
        }
        Item::Job(j) => {
            format_job(out, j, indent);
        }
    }
}

fn format_job(out: &mut String, job: &JobBlock, indent: usize) {
    out.push_str(&format!("job {} {{\n", job.key.raw));

    // Description first if present
    for dob in &job.directives {
        if let DirectiveOrBlock::Directive(d) = dob
            && d.key.value == "description"
        {
            write_indent(out, indent + 1);
            out.push_str("description ");
            out.push_str(&format_string_value(&d.args[0]));
            out.push('\n');
            out.push('\n');
            break;
        }
    }

    // Schedule
    if let Some(ref sched) = job.schedule {
        format_schedule(out, sched, indent + 1);
    }

    // Other directives
    for dob in &job.directives {
        match dob {
            DirectiveOrBlock::Directive(d) if d.key.value == "description" => continue,
            _ => format_directive_or_block(out, dob, indent + 1),
        }
    }

    out.push_str("}\n");
}

fn format_schedule(out: &mut String, sched: &ScheduleNode, indent: usize) {
    write_indent(out, indent);
    match &sched.kind {
        ScheduleKind::Interval { count, unit } => {
            let unit_str = match unit {
                IntervalUnit::Seconds => "seconds",
                IntervalUnit::Minutes => "minutes",
                IntervalUnit::Hours => "hours",
            };
            out.push_str(&format!("every {count} {unit_str}"));
        }
        ScheduleKind::Daily { time } => {
            out.push_str(&format!("every day at {}", time.raw));
        }
        ScheduleKind::Weekdays { days, time } => {
            out.push_str("every ");
            out.push_str(&format_weekday_list(days));
            out.push_str(&format!(" at {}", time.raw));
        }
        ScheduleKind::Monthly { ordinals, time } => {
            out.push_str("every ");
            let ords: Vec<String> = ordinals
                .iter()
                .map(|o| match o {
                    MonthOrdinal::Day(d) => format_ordinal(*d),
                    MonthOrdinal::Last => "last".to_string(),
                })
                .collect();
            out.push_str(&ords.join(" "));
            out.push_str(&format!(" of month at {}", time.raw));
        }
        ScheduleKind::Once { at } => {
            out.push_str(&format!("once at {}", format_string_value(at)));
        }
        ScheduleKind::Disabled => {
            out.push_str("disabled");
        }
    }

    if sched.options.is_empty() {
        out.push('\n');
    } else {
        out.push_str(" {\n");
        format_directives(out, &sched.options, indent + 1);
        write_indent(out, indent);
        out.push_str("}\n");
    }
}

/// 3-letter capitalised weekday name, e.g. `Monday → "Mon"`. Used in
/// formatter output where the DSL convention since #60 is "3-letter,
/// no quotes, full names accepted on input."
pub fn weekday_short(day: Weekday) -> &'static str {
    match day {
        Weekday::Monday => "Mon",
        Weekday::Tuesday => "Tue",
        Weekday::Wednesday => "Wed",
        Weekday::Thursday => "Thu",
        Weekday::Friday => "Fri",
        Weekday::Saturday => "Sat",
        Weekday::Sunday => "Sun",
    }
}

/// Format a list of weekdays into the canonical compact DSL form.
///
/// Rules, applied in order:
///   1. Mon–Fri (any order, deduped) → `"weekday"` alias.
///   2. Sat + Sun (any order, deduped) → `"weekend"` alias.
///   3. Otherwise: dedupe + sort by week order, then collapse any
///      contiguous run of ≥ 3 days to `Start..End` and emit the rest
///      as 3-letter singletons. So `[Mon, Tue, Wed]` becomes
///      `"Mon..Wed"`, `[Mon, Wed, Fri]` stays `"Mon Wed Fri"`, and
///      `[Mon, Fri, Sat, Sun]` becomes `"Mon Fri..Sun"`.
///
/// Wrap-around collapsing (e.g. `Sat..Mon`) is intentionally **not**
/// performed — emitting an inverse range when the user typed three
/// scattered days felt surprising. Round-tripping a wrap-around input
/// (`Fri..Mon`) yields a non-collapsed list (`Mon Fri..Sun`) which is
/// still semantically equivalent and readable.
pub fn format_weekday_list(days: &[Weekday]) -> String {
    if days.is_empty() {
        return String::new();
    }
    // Dedupe via canonical-order index.
    const ORDER: [Weekday; 7] = [
        Weekday::Monday,
        Weekday::Tuesday,
        Weekday::Wednesday,
        Weekday::Thursday,
        Weekday::Friday,
        Weekday::Saturday,
        Weekday::Sunday,
    ];
    let mut present = [false; 7];
    for d in days {
        if let Some(i) = ORDER.iter().position(|x| *x == *d) {
            present[i] = true;
        }
    }
    let indices: Vec<usize> = present
        .iter()
        .enumerate()
        .filter_map(|(i, p)| if *p { Some(i) } else { None })
        .collect();

    // Aliases.
    if indices.as_slice() == [0, 1, 2, 3, 4] {
        return "weekday".into();
    }
    if indices.as_slice() == [5, 6] {
        return "weekend".into();
    }

    // Walk for runs.
    let mut parts: Vec<String> = Vec::new();
    let mut run_start = indices[0];
    let mut run_end = indices[0];
    for &i in &indices[1..] {
        if i == run_end + 1 {
            run_end = i;
        } else {
            parts.push(emit_run(&ORDER, run_start, run_end));
            run_start = i;
            run_end = i;
        }
    }
    parts.push(emit_run(&ORDER, run_start, run_end));
    parts.join(" ")
}

fn emit_run(order: &[Weekday; 7], start: usize, end: usize) -> String {
    let len = end - start + 1;
    if len >= 3 {
        format!(
            "{}..{}",
            weekday_short(order[start]),
            weekday_short(order[end])
        )
    } else {
        (start..=end)
            .map(|i| weekday_short(order[i]))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn format_ordinal(day: u8) -> String {
    let suffix = match day {
        1 | 21 | 31 => "st",
        2 | 22 => "nd",
        3 | 23 => "rd",
        _ => "th",
    };
    format!("{day}{suffix}")
}

fn format_directives(out: &mut String, directives: &[Directive], indent: usize) {
    for d in directives {
        write_indent(out, indent);
        out.push_str(&format_string_value(&d.key));
        for arg in &d.args {
            out.push(' ');
            out.push_str(&format_string_value(arg));
        }
        out.push('\n');
    }
}

fn format_directives_or_blocks(out: &mut String, items: &[DirectiveOrBlock], indent: usize) {
    for item in items {
        format_directive_or_block(out, item, indent);
    }
}

fn format_directive_or_block(out: &mut String, item: &DirectiveOrBlock, indent: usize) {
    match item {
        DirectiveOrBlock::Directive(d) => {
            write_indent(out, indent);
            out.push_str(&format_string_value(&d.key));
            for arg in &d.args {
                out.push(' ');
                out.push_str(&format_string_value(arg));
            }
            out.push('\n');
        }
        DirectiveOrBlock::Block(b) => {
            format_named_block(out, b, indent);
        }
        DirectiveOrBlock::Comment(c) => {
            write_indent(out, indent);
            out.push_str(&format!("# {}\n", c.text));
        }
    }
}

fn format_named_block(out: &mut String, block: &NamedBlock, indent: usize) {
    write_indent(out, indent);
    out.push_str(&format_string_value(&block.name));
    if let Some(ref q) = block.qualifier {
        out.push(' ');
        out.push_str(&format_string_value(q));
    }

    // Check if block is short enough for inline
    let all_directives = block
        .directives
        .iter()
        .all(|d| matches!(d, DirectiveOrBlock::Directive(_)));
    let total_args: usize = block
        .directives
        .iter()
        .map(|d| match d {
            DirectiveOrBlock::Directive(d) => 1 + d.args.len(),
            _ => 10, // force multi-line
        })
        .sum();

    if all_directives && block.directives.len() <= 4 && total_args <= 8 {
        // Inline: { key val; key val }
        out.push_str(" {");
        for (i, dob) in block.directives.iter().enumerate() {
            if let DirectiveOrBlock::Directive(d) = dob {
                if i > 0 {
                    out.push(';');
                }
                out.push(' ');
                out.push_str(&format_string_value(&d.key));
                for arg in &d.args {
                    out.push(' ');
                    out.push_str(&format_string_value(arg));
                }
            }
        }
        out.push_str(" }\n");
    } else {
        out.push_str(" {\n");
        format_directives_or_blocks(out, &block.directives, indent + 1);
        write_indent(out, indent);
        out.push_str("}\n");
    }
}

fn format_calendar_rule(out: &mut String, rule: &CalendarRule, indent: usize) {
    write_indent(out, indent);
    let kind = match rule.kind {
        CalendarRuleKind::Include => "include",
        CalendarRuleKind::Exclude => "exclude",
    };
    out.push_str(kind);
    out.push(' ');
    out.push_str(&format_string_value(&rule.rule_type));

    // Special case `weekly`: re-collapse the expanded list back to
    // 3-letter capitalised tokens, dropping quotes and emitting
    // `Mon..Fri` for runs ≥ 3. This matches the DSL convention from
    // #60 — pre-expansion the parser stored args like `["monday",
    // "tuesday", …]` (lowercase full, after PR-D's range expansion).
    let rule_type_lower = rule.rule_type.value.to_ascii_lowercase();
    if rule_type_lower == "weekly" {
        let parsed: Option<Vec<Weekday>> =
            rule.args.iter().map(|a| Weekday::parse(&a.value)).collect();
        if let Some(days) = parsed
            && !days.is_empty()
        {
            out.push(' ');
            out.push_str(&format_weekday_list(&days));
            out.push('\n');
            return;
        }
        // Fall-through if any arg failed to parse — preserve verbatim
        // so the user still sees what they wrote and can fix the typo.
    }

    for arg in &rule.args {
        out.push(' ');
        out.push_str(&format_string_value(arg));
    }
    out.push('\n');
}

fn format_string_value(val: &StringValue) -> String {
    if val.is_placeholder {
        format!("{{{}}}", val.value)
    } else if val.quoted {
        format!(
            "\"{}\"",
            val.value.replace('\\', "\\\\").replace('"', "\\\"")
        )
    } else {
        val.value.clone()
    }
}

fn write_indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    #[test]
    fn format_roundtrip() {
        let src = r#"server {
  listen :9090
  db sqlite
}

job etl:sync {
  every 15 minutes
  timeout 10m
}
"#;
        let ast = Parser::parse(src).unwrap();
        let formatted = format(&ast);
        // Should parse back without error
        Parser::parse(&formatted).unwrap();
    }

    #[test]
    fn format_job_with_runner() {
        let ast = Parser::parse(
            r#"job ops:check {
  every 5 minutes
  runner { require health-check; require eu-west }
  timeout 30s
}"#,
        )
        .unwrap();
        let formatted = format(&ast);
        assert!(formatted.contains("runner"));
        assert!(formatted.contains("require health-check"));
    }

    #[test]
    fn weekday_list_aliases_take_priority() {
        assert_eq!(
            format_weekday_list(&[
                Weekday::Monday,
                Weekday::Tuesday,
                Weekday::Wednesday,
                Weekday::Thursday,
                Weekday::Friday,
            ]),
            "weekday"
        );
        assert_eq!(
            format_weekday_list(&[Weekday::Saturday, Weekday::Sunday]),
            "weekend"
        );
        // Order of input doesn't matter.
        assert_eq!(
            format_weekday_list(&[Weekday::Sunday, Weekday::Saturday]),
            "weekend"
        );
    }

    #[test]
    fn weekday_list_collapses_runs_of_three_or_more() {
        // Three-day run → range.
        assert_eq!(
            format_weekday_list(&[Weekday::Monday, Weekday::Tuesday, Weekday::Wednesday]),
            "Mon..Wed"
        );
        // Two-day run stays uncollapsed.
        assert_eq!(
            format_weekday_list(&[Weekday::Monday, Weekday::Tuesday]),
            "Mon Tue"
        );
        // Singleton.
        assert_eq!(format_weekday_list(&[Weekday::Wednesday]), "Wed");
    }

    #[test]
    fn weekday_list_mixed_runs_and_singletons() {
        // `Mon Wed Thu Fri` → singleton + 3-run.
        assert_eq!(
            format_weekday_list(&[
                Weekday::Monday,
                Weekday::Wednesday,
                Weekday::Thursday,
                Weekday::Friday,
            ]),
            "Mon Wed..Fri"
        );
    }

    #[test]
    fn weekday_list_dedupes() {
        assert_eq!(
            format_weekday_list(&[Weekday::Monday, Weekday::Monday, Weekday::Tuesday]),
            "Mon Tue"
        );
    }

    #[test]
    fn schedule_weekdays_round_trip_to_short_form() {
        // Long-form input parses correctly, the formatter emits the
        // canonical short form, and a second parse pass yields the
        // same Weekdays list.
        let src = "job demo:k { every monday tuesday wednesday at 09:00 }";
        let ast = Parser::parse(src).unwrap();
        let formatted = format(&ast);
        assert!(formatted.contains("every Mon..Wed at 09:00"));
        // Re-parse to confirm the new short form is a valid input.
        let ast2 = Parser::parse(&formatted).unwrap();
        if let Item::Job(ref j) = ast2.items[0] {
            if let ScheduleKind::Weekdays { ref days, .. } = j.schedule.as_ref().unwrap().kind {
                assert_eq!(
                    *days,
                    vec![Weekday::Monday, Weekday::Tuesday, Weekday::Wednesday]
                );
            } else {
                panic!("expected Weekdays after round-trip");
            }
        }
    }

    #[test]
    fn calendar_weekly_round_trip_to_short_form() {
        let src = r#"calendar biz { include weekly "Mon".."Fri" }"#;
        let ast = Parser::parse(src).unwrap();
        let formatted = format(&ast);
        // "weekday" alias since Mon..Fri is the full business week.
        assert!(formatted.contains("include weekly weekday"));
        Parser::parse(&formatted).unwrap();
    }

    #[test]
    fn calendar_weekly_three_days_collapses_to_range() {
        let src = "calendar biz { include weekly Mon Tue Wed }";
        let ast = Parser::parse(src).unwrap();
        let formatted = format(&ast);
        assert!(formatted.contains("include weekly Mon..Wed"));
    }
}
