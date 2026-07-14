package conformance

import (
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
)

// TestTriggerConformance discovers every YAML case in
// ../../conformance/cases-trigger (relative to the repo root) and runs each
// as a subtest driving the SDK's trigger (producer) client. Adding a new
// YAML automatically adds a new test entry — there is no per-case literal to
// maintain, exactly like [TestConformance].
//
// The shared producer cases land via a separate change (#287). Until that
// directory exists in the checked-out tree, this test skips rather than
// failing: the Go binding's trigger runner is wired and ready, and it
// activates automatically once the cases merge to the branch under test.
func TestTriggerConformance(t *testing.T) {
	dir, ok := triggerCasesDir()
	if !ok {
		t.Skip("sdks/conformance/cases-trigger not present yet (pending #287); " +
			"the Go trigger conformance runner is wired and activates once the shared cases land")
	}

	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatalf("list trigger cases dir %s: %v", dir, err)
	}
	var files []string
	for _, e := range entries {
		if e.IsDir() || !strings.HasSuffix(e.Name(), ".yaml") {
			continue
		}
		files = append(files, e.Name())
	}
	sort.Strings(files)
	if len(files) == 0 {
		t.Skip("no trigger .yaml cases found under cases-trigger (pending #287)")
	}

	for _, name := range files {
		name := name
		t.Run(strings.TrimSuffix(name, ".yaml"), func(t *testing.T) {
			spec, err := LoadTriggerFile(filepath.Join(dir, name))
			if err != nil {
				t.Fatalf("load trigger case: %v", err)
			}
			RunTrigger(t, spec)
		})
	}
}

// triggerCasesDir resolves sdks/conformance/cases-trigger relative to this
// test's working directory, walking upward to tolerate Bazel / unusual CI
// roots (mirrors casesDir). Returns ok=false when the directory is absent.
func triggerCasesDir() (string, bool) {
	dir, err := os.Getwd()
	if err != nil {
		return "", false
	}
	for i := 0; i < 8; i++ {
		candidate := filepath.Join(dir, "sdks", "conformance", "cases-trigger")
		if info, err := os.Stat(candidate); err == nil && info.IsDir() {
			return candidate, true
		}
		dir = filepath.Dir(dir)
	}
	return "", false
}
