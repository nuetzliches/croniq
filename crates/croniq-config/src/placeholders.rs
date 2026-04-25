//! Placeholder resolution: {$VAR}, {env.VAR}, {file.PATH}, {vars.NAME}

use std::collections::HashMap;

/// Resolve placeholders in a string value.
///
/// Supported:
/// - `{$VAR}` / `{$VAR:default}` — environment variable (compile-time)
/// - `{env.VAR}` — environment variable (runtime, same as $VAR for now)
/// - `{file./path}` — file content, trimmed
/// - `{vars.NAME}` — from vars map
pub fn resolve(
    placeholder: &str,
    vars: &HashMap<String, String>,
) -> Result<String, PlaceholderError> {
    if let Some(rest) = placeholder.strip_prefix('$') {
        // {$VAR} or {$VAR:default}
        let (name, default) = if let Some((n, d)) = rest.split_once(':') {
            (n, Some(d))
        } else {
            (rest, None)
        };
        std::env::var(name).or_else(|_| {
            default
                .map(|d| d.to_string())
                .ok_or(PlaceholderError::Unresolved(format!("${name}")))
        })
    } else if let Some(var_name) = placeholder.strip_prefix("env.") {
        std::env::var(var_name).map_err(|_| PlaceholderError::Unresolved(format!("env.{var_name}")))
    } else if let Some(path) = placeholder.strip_prefix("file.") {
        std::fs::read_to_string(path)
            .map(|s| s.trim().to_string())
            .map_err(|e| PlaceholderError::FileRead(path.to_string(), e.to_string()))
    } else if let Some(var_name) = placeholder.strip_prefix("vars.") {
        vars.get(var_name)
            .cloned()
            .ok_or(PlaceholderError::Unresolved(format!("vars.{var_name}")))
    } else {
        Err(PlaceholderError::UnknownPrefix(placeholder.to_string()))
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum PlaceholderError {
    #[error("unresolved placeholder '{0}'")]
    Unresolved(String),

    #[error("cannot read file '{0}': {1}")]
    FileRead(String, String),

    #[error("unknown placeholder prefix: '{0}'")]
    UnknownPrefix(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_vars() {
        let mut vars = HashMap::new();
        vars.insert("tz".to_string(), "Europe/Vienna".to_string());
        assert_eq!(resolve("vars.tz", &vars).unwrap(), "Europe/Vienna");
    }

    #[test]
    fn resolve_env_with_default() {
        let vars = HashMap::new();
        // Unlikely to exist
        let result = resolve("$CRONIQ_TEST_NONEXISTENT:fallback", &vars);
        assert_eq!(result.unwrap(), "fallback");
    }

    #[test]
    fn unresolved_error() {
        let vars = HashMap::new();
        let result = resolve("vars.nope", &vars);
        assert!(result.is_err());
    }
}
