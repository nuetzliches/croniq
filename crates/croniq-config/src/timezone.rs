//! IANA timezone resolution, shared by the validator, the CLI and the
//! server's loader (issue #426).
//!
//! Before this module existed every consumer wrote
//! `s.parse().ok().unwrap_or(chrono_tz::UTC)`, so a one-character typo in
//! `timezone Europe/Berln` passed `validate`, survived `compile`, and moved
//! every wall-clock fire of that job by the zone's offset — permanently, and
//! without a single line in the log. A misconfigured zone is now a hard
//! failure everywhere: [`parse`] is the only way a zone name becomes a
//! [`chrono_tz::Tz`] in this workspace.
//!
//! The error carries a did-you-mean suggestion built from the same edit
//! distance the unknown-directive diagnostics use (issue #403), because the
//! realistic mistake is a typo in a name the operator already knows.

use chrono_tz::Tz;

/// A `timezone` value that is not an IANA zone name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidTimezone {
    /// The offending value, as written in the config.
    pub value: String,
    /// Nearest known zone name, when the value looks like a typo of one.
    pub suggestion: Option<&'static str>,
}

impl std::fmt::Display for InvalidTimezone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown timezone '{}'", self.value)?;
        match self.suggestion {
            Some(best) => write!(f, " — did you mean '{best}'?"),
            None => write!(
                f,
                " — expected an IANA zone name such as 'Europe/Vienna', \
                 'America/New_York' or 'UTC'"
            ),
        }
    }
}

impl std::error::Error for InvalidTimezone {}

/// Parse an IANA zone name (`Europe/Vienna`, `UTC`, …).
///
/// Deliberately strict: there is no fallback to UTC here, because a silent
/// fallback is exactly the bug this module exists to prevent. Callers that
/// legitimately have no zone (none declared anywhere) choose UTC themselves,
/// which is the documented default — see issue #427.
pub fn parse(value: &str) -> Result<Tz, InvalidTimezone> {
    value.parse::<Tz>().map_err(|_| InvalidTimezone {
        value: value.to_string(),
        suggestion: closest_zone(value),
    })
}

/// Whether `value` names a known IANA zone.
pub fn is_valid(value: &str) -> bool {
    value.parse::<Tz>().is_ok()
}

/// Nearest known zone name within a small edit distance, so `Europe/Berln`
/// suggests `Europe/Berlin` but `not-a-zone` suggests nothing. Compared
/// case-insensitively: chrono-tz itself parses zone names case-sensitively,
/// so `europe/vienna` is an error whose best fix is the canonical spelling.
fn closest_zone(value: &str) -> Option<&'static str> {
    let budget = match value.chars().count() {
        0..=3 => 1,
        4..=8 => 2,
        _ => 3,
    };
    let needle = value.to_ascii_lowercase();
    chrono_tz::TZ_VARIANTS
        .iter()
        .map(|tz| tz.name())
        .map(|name| {
            (
                name,
                crate::block_directives::edit_distance(&needle, &name.to_ascii_lowercase()),
            )
        })
        .filter(|(_, d)| *d <= budget)
        .min_by_key(|(name, d)| (*d, name.len()))
        .map(|(name, _)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_zones_parse() {
        assert_eq!(parse("UTC").unwrap(), Tz::UTC);
        assert_eq!(parse("Europe/Vienna").unwrap(), Tz::Europe__Vienna);
        assert!(is_valid("America/New_York"));
    }

    #[test]
    fn typo_suggests_the_intended_zone() {
        let err = parse("Europe/Berln").unwrap_err();
        assert_eq!(err.suggestion, Some("Europe/Berlin"));
        assert!(
            err.to_string().contains("did you mean 'Europe/Berlin'?"),
            "got: {err}"
        );
    }

    #[test]
    fn wrong_case_suggests_the_canonical_spelling() {
        // chrono-tz parses zone names case-sensitively, so this is invalid —
        // but it is one of the most likely mistakes, and the fix is a spelling.
        let err = parse("europe/vienna").unwrap_err();
        assert_eq!(err.suggestion, Some("Europe/Vienna"));
    }

    #[test]
    fn nonsense_lists_the_expected_shape_instead_of_guessing() {
        let err = parse("not-a-timezone-at-all").unwrap_err();
        assert_eq!(err.suggestion, None);
        assert!(err.to_string().contains("IANA zone name"), "got: {err}");
    }

    #[test]
    fn empty_value_is_invalid() {
        assert!(!is_valid(""));
    }
}
