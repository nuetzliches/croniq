//! Duration-string parsing shared by the binary and the library.
//!
//! Lived in `main.rs` while only the boot path needed it. The API-key
//! rotation grace window (`CRONIQ_API_KEY_ROTATION_GRACE`) is read from
//! `init_api_key`, which is library code, so the parser moved here rather
//! than being written a second time with subtly different rules.

/// Parse a duration string like `"60s"`, `"2m"`, `"1h"`, or a bare integer
/// (interpreted as seconds) into seconds. Returns an error string on malformed
/// input rather than silently falling back, so that bad config surfaces at boot
/// instead of becoming a 2-minute lease nobody asked for.
pub fn parse_duration_secs(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let parse = |body: &str, mult: u64, suffix: char| -> Result<u64, String> {
        body.parse::<u64>()
            .map_err(|_| format!("invalid duration {s:?}: cannot parse number before '{suffix}'"))
            .and_then(|v| {
                v.checked_mul(mult)
                    .ok_or_else(|| format!("duration {s:?} overflows u64 seconds"))
            })
    };
    if let Some(n) = s.strip_suffix('s') {
        parse(n, 1, 's')
    } else if let Some(n) = s.strip_suffix('m') {
        parse(n, 60, 'm')
    } else if let Some(n) = s.strip_suffix('h') {
        parse(n, 3600, 'h')
    } else {
        s.parse::<u64>()
            .map_err(|_| format!("invalid duration {s:?}: expected '<n>[s|m|h]' or bare seconds"))
    }
}

#[cfg(test)]
mod tests {
    use super::parse_duration_secs;

    #[test]
    fn parses_units_and_bare_seconds() {
        assert_eq!(parse_duration_secs("30s").unwrap(), 30);
        assert_eq!(parse_duration_secs("2m").unwrap(), 120);
        assert_eq!(parse_duration_secs("1h").unwrap(), 3600);
        assert_eq!(parse_duration_secs("45").unwrap(), 45);
        assert_eq!(parse_duration_secs("  10s  ").unwrap(), 10);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_duration_secs("abc").is_err());
        assert!(parse_duration_secs("10x").is_err());
        assert!(parse_duration_secs("ms").is_err());
        assert!(parse_duration_secs("").is_err());
    }
}
