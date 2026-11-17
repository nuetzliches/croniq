use croniq_config::{compile, convert, diff, format, parser, validate};
use miette::{IntoDiagnostic, Result, WrapErr};
use std::path::Path;

/// The configuration the server would load from `path`: the file with its
/// `import`s inlined (review 2026-11-17, finding 3). Checked on its own, a
/// file that leans on an imported calendar, `concurrency_group` or
/// `defaults { }` failed although the server accepts it.
///
/// Each import the server would skip is printed as an **error**, and the
/// count returned for the caller's summary and exit code. The server only
/// warns and loads the rest, so the jobs of a broken import go missing with
/// nothing failing; `validate` and `compile` are the gate before a deploy,
/// which is where that has to stop.
fn load_resolved(path: &Path) -> Result<(croniq_config::ast::Croniqfile, usize)> {
    use croniq_config::import::{LoadFileError, parse_file_resolved};

    let resolved = parse_file_resolved(path).map_err(|e| match e {
        LoadFileError::Io(e) => miette::miette!("cannot read {}: {e}", path.display()),
        LoadFileError::Parse { text, error } => miette::Report::new(error)
            .with_source_code(miette::NamedSource::new(path.display().to_string(), text)),
    })?;
    for skipped in &resolved.skipped {
        eprintln!("error: {skipped} — the server would load the file without it");
    }
    Ok((resolved.ast, resolved.skipped.len()))
}

pub fn validate(path: &Path) -> Result<()> {
    let (ast, skipped_imports) = load_resolved(path)?;

    let diags = validate::validate(&ast);
    let errors = diags
        .iter()
        .filter(|d| d.severity == validate::Severity::Error)
        .count()
        + skipped_imports;
    // Placeholders resolve against *this* shell's environment and files, which
    // need not be the server's — so a warning, not an error (review
    // 2026-10-24, F-14).
    let unresolved = compile::unresolved_placeholders(&ast);
    let warnings = diags
        .iter()
        .filter(|d| d.severity == validate::Severity::Warning)
        .count()
        + unresolved.len();

    for diag in &diags {
        let prefix = match diag.severity {
            validate::Severity::Error => "error",
            validate::Severity::Warning => "warning",
        };
        eprintln!("{prefix}: {}", diag.message);
    }
    for msg in &unresolved {
        eprintln!(
            "warning: {msg} in this environment — the server would use the placeholder's \
             literal text"
        );
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
    let (ast, skipped_imports) = load_resolved(path)?;

    // Compiling a file the server would refuse to load must not print JSON that
    // looks authoritative (issue #426): `compile` output is meant to be the
    // complete description of the schedule, and a `timezone Europe/Berln` used
    // to sail through into it verbatim. Error-severity diagnostics are exactly
    // what `load_from_compiled` fails closed on since #402, so the two agree.
    // Warnings go to stderr and still produce output — stdout stays pure JSON.
    let diags = validate::validate(&ast);
    // A skipped import leaves its jobs out of the JSON, which would then not
    // be the complete description it claims to be.
    let mut errors = skipped_imports;
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
