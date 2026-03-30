//! Import resolution: expands `import ./path` and `import ./glob/*.croniq` directives.

use std::path::{Path, PathBuf};

/// Resolve import paths relative to the base directory.
/// Supports glob patterns.
pub fn resolve_imports(
    base_dir: &Path,
    import_path: &str,
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
