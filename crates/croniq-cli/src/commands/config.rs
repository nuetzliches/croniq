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
