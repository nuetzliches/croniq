use croniq_config::{compile, convert, diff, format, parser, validate};
use miette::{IntoDiagnostic, Result, WrapErr};
use std::path::Path;

pub fn validate(path: &Path) -> Result<()> {
    let source = std::fs::read_to_string(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("cannot read {}", path.display()))?;

    let ast = parser::Parser::parse(&source).map_err(|e| {
        miette::Report::new(e).with_source_code(miette::NamedSource::new(
            path.display().to_string(),
            source.clone(),
        ))
    })?;

    let diags = validate::validate(&ast);
    let errors = diags
        .iter()
        .filter(|d| d.severity == validate::Severity::Error)
        .count();
    let warnings = diags
        .iter()
        .filter(|d| d.severity == validate::Severity::Warning)
        .count();

    for diag in &diags {
        let prefix = match diag.severity {
            validate::Severity::Error => "error",
            validate::Severity::Warning => "warning",
        };
        eprintln!("{prefix}: {}", diag.message);
    }

    if errors > 0 {
        eprintln!("\n{errors} error(s), {warnings} warning(s)");
        std::process::exit(1);
    } else if warnings > 0 {
        eprintln!("\n{warnings} warning(s)");
    } else {
        let job_count = ast
            .items
            .iter()
            .filter(|i| matches!(i, croniq_config::ast::Item::Job(_)))
            .count();
        eprintln!("ok: {job_count} job(s), 0 errors");
    }

    Ok(())
}

pub fn fmt(path: &Path, write: bool) -> Result<()> {
    let source = std::fs::read_to_string(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("cannot read {}", path.display()))?;

    let ast = parser::Parser::parse(&source).map_err(|e| {
        miette::Report::new(e).with_source_code(miette::NamedSource::new(
            path.display().to_string(),
            source.clone(),
        ))
    })?;

    let formatted = format::format(&ast);

    // The formatter keeps comments only where the AST does (top level, job
    // bodies, `defaults { }`). Overwriting the file would delete the rest, so
    // `--write` refuses instead (review 2026-10-24, F-7); printing to stdout
    // stays allowed, with the same warning.
    let lost = format::lost_comments(&source, &formatted);
    if !lost.is_empty() {
        let shown: Vec<String> = lost.iter().take(5).map(|c| format!("  # {c}")).collect();
        let more = if lost.len() > 5 {
            format!("\n  … and {} more", lost.len() - 5)
        } else {
            String::new()
        };
        let msg = format!(
            "formatting {} would drop {} comment(s) the formatter cannot keep \
             (comments inside blocks other than jobs and `defaults {{ }}`):\n{}{more}",
            path.display(),
            lost.len(),
            shown.join("\n")
        );
        if write {
            return Err(miette::miette!(
                "{msg}\nnot written — move those comments above their block, or run \
                 `croniq fmt` without --write and apply the output by hand"
            ));
        }
        eprintln!("warning: {msg}");
    }

    if write {
        std::fs::write(path, &formatted)
            .into_diagnostic()
            .wrap_err_with(|| format!("cannot write {}", path.display()))?;
        eprintln!("formatted {}", path.display());
    } else {
        print!("{formatted}");
    }

    Ok(())
}

pub fn compile(path: &Path) -> Result<()> {
    let source = std::fs::read_to_string(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("cannot read {}", path.display()))?;

    let ast = parser::Parser::parse(&source).map_err(|e| {
        miette::Report::new(e).with_source_code(miette::NamedSource::new(
            path.display().to_string(),
            source.clone(),
        ))
    })?;

    // Compiling a file the server would refuse to load must not print JSON that
    // looks authoritative (issue #426): `compile` output is meant to be the
    // complete description of the schedule, and a `timezone Europe/Berln` used
    // to sail through into it verbatim. Error-severity diagnostics are exactly
    // what `load_from_compiled` fails closed on since #402, so the two agree.
    // Warnings go to stderr and still produce output — stdout stays pure JSON.
    let diags = validate::validate(&ast);
    let mut errors = 0usize;
    for diag in &diags {
        match diag.severity {
            validate::Severity::Error => {
                errors += 1;
                eprintln!("error: {}", diag.message);
            }
            validate::Severity::Warning => eprintln!("warning: {}", diag.message),
        }
    }
    if errors > 0 {
        eprintln!(
            "\n{errors} error(s) — nothing compiled. Run `croniq validate` for the full list."
        );
        std::process::exit(1);
    }

    let config = compile::compile(&ast);
    let json = serde_json::to_string_pretty(&config).into_diagnostic()?;
    println!("{json}");

    Ok(())
}

pub fn diff(old_path: &Path, new_path: &Path) -> Result<()> {
    let old_source = std::fs::read_to_string(old_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("cannot read {}", old_path.display()))?;
    let new_source = std::fs::read_to_string(new_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("cannot read {}", new_path.display()))?;

    let d = diff::diff(
        &old_source,
        &new_source,
        &old_path.display().to_string(),
        &new_path.display().to_string(),
    );

    if d.is_empty() {
        eprintln!("files are identical");
    } else {
        print!("{d}");
    }

    Ok(())
}

pub fn convert_cron(expr: &str) -> Result<()> {
    match convert::convert(expr) {
        Ok(result) => {
            println!("{}", result.schedule);
            for warning in &result.warnings {
                eprintln!("warning: {warning}");
            }
            Ok(())
        }
        Err(msg) => Err(miette::miette!("{msg}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str, contents: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("croniq-fmt-{}-{name}.croniq", std::process::id()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn fmt_write_refuses_to_drop_comments() {
        // Review 2026-10-24, F-7: comments inside `server { }` are not in the
        // AST, and `--write` used to delete them.
        let src = "server {\n  # keep me\n  listen :4000\n}\n";
        let path = scratch("lossy", src);
        let err = fmt(&path, true).expect_err("must refuse");
        assert!(err.to_string().contains("keep me"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            src,
            "file untouched"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn fmt_write_proceeds_when_every_comment_survives() {
        let src = "# top\njob a:b {\n  # in job\n  every 5 minutes\n}\n";
        let path = scratch("lossless", src);
        fmt(&path, true).expect("nothing to lose");
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(
            written.contains("# top") && written.contains("# in job"),
            "{written}"
        );
        std::fs::remove_file(&path).ok();
    }
}
