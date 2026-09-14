//! Render an error with everything underneath it.
//!
//! `Display` on a `reqwest::Error` is "error sending request for url (…)" —
//! the URL, and nothing about why. Whether the connection was refused, reset
//! by the peer, closed mid-response, or timed out is in the source chain,
//! which `{}` does not walk. The runner logged the top line and threw the rest
//! away, which is how issue #648 stayed a mystery: a warning that fires
//! regularly and names no cause is one operators learn to scroll past.
//!
//! `anyhow` would do this, and the SDK deliberately does not depend on it:
//! this is a library other people link into their runners, and a public error
//! type should be `thiserror` + `std::error::Error` rather than dragging a
//! reporting crate into someone else's dependency graph for one formatting
//! helper.

use std::error::Error;
use std::fmt::Write as _;

/// The error, then each `source()` beneath it, separated by `: `.
///
/// Repeated links are collapsed. Some stacks (hyper through reqwest) restate
/// the same sentence at two levels, and "error sending request: error sending
/// request: connection closed" reads like a bug in the logger.
pub fn chain(err: &dyn Error) -> String {
    let mut out = err.to_string();
    let mut last = out.clone();
    let mut source = err.source();

    // A bound rather than `while let`: a cyclic `source()` chain is legal as
    // far as the type system is concerned, and a logger is the last place that
    // should be able to hang.
    for _ in 0..8 {
        let Some(cause) = source else { break };
        let text = cause.to_string();
        if text != last && !last.contains(&text) {
            let _ = write!(out, ": {text}");
            last = text;
        }
        source = cause.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;

    #[derive(Debug)]
    struct Layer {
        text: &'static str,
        below: Option<Box<Layer>>,
    }

    impl fmt::Display for Layer {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.text)
        }
    }

    impl Error for Layer {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.below.as_deref().map(|l| l as &(dyn Error + 'static))
        }
    }

    fn stack(texts: &[&'static str]) -> Layer {
        let mut it = texts.iter().rev();
        let mut node = Layer {
            text: it.next().unwrap(),
            below: None,
        };
        for t in it {
            node = Layer {
                text: t,
                below: Some(Box::new(node)),
            };
        }
        node
    }

    #[test]
    fn a_single_error_is_unchanged() {
        assert_eq!(chain(&stack(&["boom"])), "boom");
    }

    #[test]
    fn the_whole_chain_is_rendered() {
        assert_eq!(
            chain(&stack(&[
                "HTTP error",
                "error sending request",
                "connection closed"
            ])),
            "HTTP error: error sending request: connection closed"
        );
    }

    /// The case that motivated the dedupe: hyper's error restated by reqwest.
    #[test]
    fn a_repeated_link_is_not_printed_twice() {
        assert_eq!(
            chain(&stack(&[
                "error sending request",
                "error sending request",
                "reset"
            ])),
            "error sending request: reset"
        );
    }

    /// `Display` impls that already embed their cause — `thiserror`'s
    /// `#[error("… {0}")]` does exactly this — would otherwise print it twice.
    #[test]
    fn a_cause_already_quoted_by_its_parent_is_not_repeated() {
        assert_eq!(
            chain(&stack(&[
                "HTTP error: connection closed",
                "connection closed"
            ])),
            "HTTP error: connection closed"
        );
    }

    /// A logger must not be the thing that hangs the process.
    #[test]
    fn a_very_deep_chain_is_bounded() {
        let deep: Vec<&'static str> = vec!["a"; 40];
        assert!(chain(&stack(&deep)).len() < 64);
    }
}
