//! Grammatical number for schedule intervals.
//!
//! Every schedule emitter — [`format`](crate::format), the
//! [`schedule`](crate::schedule) summary, and the
//! [`convert`](crate::convert) cron translator — has to render an
//! interval as `<count> <unit>` with the unit noun in the right number.
//! Keeping the count-of-1 rule in one place stops them drifting apart
//! (they did: `convert` kept emitting `every 1 minutes` after issue #336
//! fixed the others).

/// Format an interval as `<count> <unit>` with grammatical number: the
/// bare singular unit noun for a count of 1 (`"1 minute"`), the `+s`
/// plural otherwise (`"5 minutes"`, `"0 minutes"`).
///
/// `singular_unit` is the singular noun (`"second"`, `"minute"`,
/// `"hour"`); the plural is formed by appending `s`.
pub fn interval_phrase(count: u64, singular_unit: &str) -> String {
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {singular_unit}{plural}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_of_one_is_singular() {
        assert_eq!(interval_phrase(1, "minute"), "1 minute");
        assert_eq!(interval_phrase(1, "hour"), "1 hour");
        assert_eq!(interval_phrase(1, "second"), "1 second");
    }

    #[test]
    fn other_counts_are_plural() {
        assert_eq!(interval_phrase(5, "minute"), "5 minutes");
        assert_eq!(interval_phrase(2, "hour"), "2 hours");
        // Zero pluralises, matching English usage.
        assert_eq!(interval_phrase(0, "second"), "0 seconds");
    }
}
