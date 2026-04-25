//! Import resolution: expands `import ./path` and `import ./glob/*.croniq` directives.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
