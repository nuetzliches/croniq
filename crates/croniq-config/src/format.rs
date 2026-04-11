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
            && d.key.value == "description" {
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
            let day_names: Vec<&str> = days.iter().map(|d| d.as_str()).collect();
            out.push_str(&day_names.join(" "));
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
        format!("\"{}\"", val.value.replace('\\', "\\\\").replace('"', "\\\""))
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
}
