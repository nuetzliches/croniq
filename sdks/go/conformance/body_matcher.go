package conformance

import (
	"encoding/json"
	"fmt"
)

// MatchBody asserts that `actual` (raw JSON bytes) satisfies the subset
// rules in `expected` (parsed from YAML — map[string]any / []any /
// scalars). Returns nil on success, or a human-readable path-rooted diff
// for the first mismatch found.
//
// One wildcard token is supported: a literal string `"*"` matches any
// non-empty value of any JSON kind. Subset semantics — keys in the
// actual body that aren't mentioned in `expected` are ignored. Array
// length must match.
func MatchBody(expected any, actualJSON string) string {
	if expected == nil {
		return ""
	}
	var actual any
	if actualJSON == "" {
		return "$: empty actual body"
	}
	if err := json.Unmarshal([]byte(actualJSON), &actual); err != nil {
		return fmt.Sprintf("$: not valid JSON: %v", err)
	}
	return match(expected, actual, "$")
}

func match(expected, actual any, path string) string {
	if expected == nil {
		if actual == nil {
			return ""
		}
		return fmt.Sprintf("%s: expected null but got %T", path, actual)
	}

	if s, ok := expected.(string); ok && s == "*" {
		// non-empty wildcard
		switch v := actual.(type) {
		case nil:
			return fmt.Sprintf("%s: expected non-empty wildcard match but got null", path)
		case string:
			if v == "" {
				return fmt.Sprintf("%s: expected non-empty string but got empty", path)
			}
		}
		return ""
	}

	switch e := expected.(type) {
	case map[string]any:
		am, ok := actual.(map[string]any)
		if !ok {
			return fmt.Sprintf("%s: expected object but got %T", path, actual)
		}
		for k, v := range e {
			child, present := am[k]
			if !present {
				return fmt.Sprintf("%s.%s: missing key", path, k)
			}
			if msg := match(v, child, path+"."+k); msg != "" {
				return msg
			}
		}
		return ""

	case []any:
		al, ok := actual.([]any)
		if !ok {
			return fmt.Sprintf("%s: expected array but got %T", path, actual)
		}
		if len(al) != len(e) {
			return fmt.Sprintf("%s: expected %d item(s) but got %d", path, len(e), len(al))
		}
		for i := range e {
			if msg := match(e[i], al[i], fmt.Sprintf("%s[%d]", path, i)); msg != "" {
				return msg
			}
		}
		return ""

	case string:
		s, ok := actual.(string)
		if !ok {
			return fmt.Sprintf("%s: expected string %q but got %T", path, e, actual)
		}
		if s != e {
			return fmt.Sprintf("%s: expected %q but got %q", path, e, s)
		}
		return ""

	case bool:
		b, ok := actual.(bool)
		if !ok {
			return fmt.Sprintf("%s: expected bool but got %T", path, actual)
		}
		if b != e {
			return fmt.Sprintf("%s: expected %v but got %v", path, e, b)
		}
		return ""

	case int:
		return matchNumber(float64(e), actual, path)
	case int64:
		return matchNumber(float64(e), actual, path)
	case float64:
		return matchNumber(e, actual, path)

	default:
		return fmt.Sprintf("%s: unsupported expected type %T", path, expected)
	}
}

func matchNumber(expected float64, actual any, path string) string {
	switch n := actual.(type) {
	case float64:
		if n != expected {
			return fmt.Sprintf("%s: expected %v but got %v", path, expected, n)
		}
		return ""
	case int:
		if float64(n) != expected {
			return fmt.Sprintf("%s: expected %v but got %v", path, expected, n)
		}
		return ""
	default:
		return fmt.Sprintf("%s: expected number but got %T", path, actual)
	}
}
