package conformance

import (
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
)

// TestConformance discovers every YAML case in ../../conformance/cases
// (relative to the repo root) and runs each as a subtest. Adding a new
// YAML automatically adds a new test entry — there is intentionally no
// per-case `t.Run("X", …)` literal to maintain.
func TestConformance(t *testing.T) {
	dir := casesDir(t)
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatalf("list cases dir %s: %v", dir, err)
	}
	var files []string
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		if !strings.HasSuffix(e.Name(), ".yaml") {
			continue
		}
		files = append(files, e.Name())
	}
	sort.Strings(files)
	if len(files) == 0 {
		t.Fatalf("no .yaml cases found under %s", dir)
	}

	for _, name := range files {
		name := name
		t.Run(strings.TrimSuffix(name, ".yaml"), func(t *testing.T) {
			spec, err := LoadFile(filepath.Join(dir, name))
			if err != nil {
				t.Fatalf("load case: %v", err)
			}
			Run(t, spec)
		})
	}
}

// casesDir resolves sdks/conformance/cases relative to this test file.
// The Go binding sits at sdks/go/conformance, so the shared cases live
// two levels up.
func casesDir(t *testing.T) string {
	t.Helper()
	// Walk upward from the test working directory looking for the
	// sentinel file. Bazel / weird CI roots are handled naturally.
	dir, err := os.Getwd()
	if err != nil {
		t.Fatalf("getwd: %v", err)
	}
	for i := 0; i < 8; i++ {
		candidate := filepath.Join(dir, "sdks", "conformance", "cases")
		if _, err := os.Stat(candidate); err == nil {
			return candidate
		}
		dir = filepath.Dir(dir)
	}
	t.Fatalf("could not locate sdks/conformance/cases relative to working dir")
	return ""
}
