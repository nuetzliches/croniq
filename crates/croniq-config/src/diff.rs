//! Unified diff between two Croniqfile sources.

use similar::TextDiff;

/// Generate a unified diff between two Croniqfile sources.
pub fn diff(old: &str, new: &str, old_name: &str, new_name: &str) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut output = String::new();

    for hunk in diff.unified_diff().header(old_name, new_name).iter_hunks() {
        output.push_str(&format!("{hunk}"));
    }

    output
}

/// Returns true if two Croniqfile sources are semantically equivalent
/// (ignoring formatting differences).
pub fn is_equivalent(a: &str, b: &str) -> bool {
    use crate::format;
    use crate::parser::Parser;

    let ast_a = match Parser::parse(a) {
        Ok(ast) => ast,
        Err(_) => return false,
    };
    let ast_b = match Parser::parse(b) {
        Ok(ast) => ast,
        Err(_) => return false,
    };

    // Compare by normalizing through formatter
    let fmt_a = format::format(&ast_a);
    let fmt_b = format::format(&ast_b);
    fmt_a == fmt_b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_identical() {
        let src = "server { listen :9090 }";
        assert!(diff(src, src, "a", "b").is_empty());
    }

    #[test]
    fn diff_shows_changes() {
        let old = "server {\n  listen :9090\n}\n";
        let new = "server {\n  listen :8080\n}\n";
        let d = diff(old, new, "old", "new");
        assert!(d.contains("-  listen :9090"));
        assert!(d.contains("+  listen :8080"));
    }
}
