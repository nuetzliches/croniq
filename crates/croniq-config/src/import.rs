//! Import resolution: expands `import ./path` and `import ./glob/*.croniq` directives.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::ast::{Croniqfile, Item};
use crate::parser::{ParseError, Parser};

/// Resolve import paths relative to the base directory.
/// Supports glob patterns. Detects circular imports via a visited set of
/// canonical paths.
pub fn resolve_imports(base_dir: &Path, import_path: &str) -> Result<Vec<PathBuf>, ImportError> {
    resolve_imports_with_visited(base_dir, import_path, &mut HashSet::new())
}

/// Inner resolver that tracks visited canonical paths to detect cycles.
pub fn resolve_imports_with_visited(
    base_dir: &Path,
    import_path: &str,
    visited: &mut HashSet<PathBuf>,
) -> Result<Vec<PathBuf>, ImportError> {
    let full_pattern = base_dir.join(import_path);
    let pattern_str = full_pattern
        .to_str()
        .ok_or_else(|| ImportError::InvalidPath(import_path.to_string()))?;

    let mut paths: Vec<PathBuf> = glob::glob(pattern_str)
        .map_err(|e| ImportError::GlobError(pattern_str.to_string(), e.to_string()))?
        .filter_map(|entry| entry.ok())
        .filter(|p| p.is_file())
        .collect();

    paths.sort();

    if paths.is_empty() {
        return Err(ImportError::NoMatches(import_path.to_string()));
    }

    // Check for circular imports using canonical paths
    for path in &paths {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !visited.insert(canonical.clone()) {
            return Err(ImportError::CircularImport(path.display().to_string()));
        }
    }

    Ok(paths)
}

/// A Croniqfile with every `import` replaced by the items it names, in place
/// and recursively — the configuration the server runs.
#[derive(Debug)]
pub struct ResolvedFile {
    pub ast: Croniqfile,
    /// Imports left out, in the order met. The server skips each with a
    /// warning rather than refusing the boot, so a caller that checks a file
    /// must say what it did not see.
    pub skipped: Vec<SkippedImport>,
}

/// An `import` [`parse_file_resolved`] could not inline.
#[derive(Debug)]
pub enum SkippedImport {
    /// The pattern matched no file, or a matched file closes a cycle.
    Unresolved { import: String, error: ImportError },
    /// A matched file could not be read or does not parse.
    Unreadable { file: PathBuf, error: LoadFileError },
}

impl std::fmt::Display for SkippedImport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unresolved { import, error } => write!(f, "import '{import}' skipped: {error}"),
            Self::Unreadable { file, error } => {
                write!(f, "imported file '{}' skipped: {error}", file.display())
            }
        }
    }
}

/// Why the top-level file of [`parse_file_resolved`] could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum LoadFileError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// Carries the file's text, which a caller needs to point at the error.
    #[error("{error}")]
    Parse { text: String, error: ParseError },
}

/// Parse `path` and inline its imports recursively.
///
/// The one implementation of what a Croniqfile *is*: the server loads with
/// it, and `croniq validate` / `croniq compile` check with it. When the CLI
/// checked the top-level file alone, a calendar, `concurrency_group` or
/// `defaults { }` that an import supplies read as missing (review
/// 2026-11-17, finding 3). Only the top-level file is fatal; an import that
/// does not resolve or parse is left out and listed in
/// [`ResolvedFile::skipped`].
pub fn parse_file_resolved(path: &Path) -> Result<ResolvedFile, LoadFileError> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut visited = HashSet::from([canonical.clone()]);
    let mut skipped = Vec::new();
    let ast = parse_and_inline(&canonical, &mut visited, &mut skipped)?;
    Ok(ResolvedFile { ast, skipped })
}

fn parse_and_inline(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    skipped: &mut Vec<SkippedImport>,
) -> Result<Croniqfile, LoadFileError> {
    let text = std::fs::read_to_string(path)?;
    let mut ast = match Parser::parse(&text) {
        Ok(ast) => ast,
        Err(error) => return Err(LoadFileError::Parse { text, error }),
    };
    let base_dir = path.parent().unwrap_or(Path::new("."));

    let mut items = Vec::with_capacity(ast.items.len());
    for item in std::mem::take(&mut ast.items) {
        let Item::Import(ref import) = item else {
            items.push(item);
            continue;
        };
        let pattern = &import.path.value;
        match resolve_imports_with_visited(base_dir, pattern, visited) {
            Ok(files) => {
                for file in files {
                    match parse_and_inline(&file, visited, skipped) {
                        Ok(imported) => items.extend(imported.items),
                        Err(error) => skipped.push(SkippedImport::Unreadable { file, error }),
                    }
                }
            }
            Err(error) => skipped.push(SkippedImport::Unresolved {
                import: pattern.clone(),
                error,
            }),
        }
    }
    ast.items = items;
    Ok(ast)
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ImportError {
    #[error("invalid import path: '{0}'")]
    InvalidPath(String),

    #[error("glob error for '{0}': {1}")]
    GlobError(String, String),

    #[error("no files match import pattern '{0}'")]
    NoMatches(String),

    #[error("circular import detected: '{0}'")]
    CircularImport(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn circular_import_detected() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.croniq");
        let file_b = dir.path().join("b.croniq");
        fs::write(&file_a, "import ./b.croniq").unwrap();
        fs::write(&file_b, "import ./a.croniq").unwrap();

        let mut visited = HashSet::new();

        // First resolve: a.croniq → visited = {a.croniq}
        let result = resolve_imports_with_visited(dir.path(), "a.croniq", &mut visited);
        assert!(result.is_ok());

        // Second resolve: a.croniq again → circular!
        let result = resolve_imports_with_visited(dir.path(), "a.croniq", &mut visited);
        assert!(matches!(result, Err(ImportError::CircularImport(_))));
    }

    #[test]
    fn non_circular_imports_pass() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.croniq");
        let file_b = dir.path().join("b.croniq");
        fs::write(&file_a, "job a { every 1 hours }").unwrap();
        fs::write(&file_b, "job b { every 1 hours }").unwrap();

        let mut visited = HashSet::new();

        let result_a = resolve_imports_with_visited(dir.path(), "a.croniq", &mut visited);
        assert!(result_a.is_ok());

        let result_b = resolve_imports_with_visited(dir.path(), "b.croniq", &mut visited);
        assert!(result_b.is_ok());
    }
}
