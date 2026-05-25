package croniq

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestGenerateIDFormat(t *testing.T) {
	id := generateID("shell-runner")
	if !strings.HasPrefix(id, "shell-runner-") {
		t.Errorf("missing prefix in %q", id)
	}
	suffix := strings.TrimPrefix(id, "shell-runner-")
	if len(suffix) != 8 {
		t.Errorf("suffix length = %d, want 8", len(suffix))
	}
	for _, c := range suffix {
		if !isHex(c) {
			t.Errorf("non-hex char in suffix: %q", suffix)
			break
		}
	}
}

func isHex(c rune) bool {
	return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f')
}

func TestPersistAndReuseRunnerID(t *testing.T) {
	dir := t.TempDir()
	id1 := resolveOrPersist("shell-runner", dir)
	id2 := resolveOrPersist("shell-runner", dir)
	if id1 != id2 {
		t.Errorf("second resolve should reuse persisted id; got %q != %q", id1, id2)
	}
	on_disk, err := os.ReadFile(filepath.Join(dir, stateFileName))
	if err != nil {
		t.Fatalf("read state file: %v", err)
	}
	if strings.TrimSpace(string(on_disk)) != id1 {
		t.Errorf("on-disk %q != returned %q", on_disk, id1)
	}
}

func TestReadsExistingStateFileVerbatim(t *testing.T) {
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, stateFileName), []byte("shell-runner-vps-prod\n"), 0o644); err != nil {
		t.Fatalf("write seed: %v", err)
	}
	id := resolveOrPersist("shell-runner", dir)
	if id != "shell-runner-vps-prod" {
		t.Errorf("got %q, want shell-runner-vps-prod", id)
	}
}

func TestIgnoresEmptyStateFileAndRegenerates(t *testing.T) {
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, stateFileName), []byte("  \n"), 0o644); err != nil {
		t.Fatalf("write seed: %v", err)
	}
	id := resolveOrPersist("shell-runner", dir)
	if !strings.HasPrefix(id, "shell-runner-") || strings.TrimSpace(id) == "" {
		t.Errorf("expected fresh prefixed id; got %q", id)
	}
}

func TestFallsBackOnUnwritableDir(t *testing.T) {
	// /proc/<x> is read-only on Linux. On platforms where this path is
	// writable the test still passes, we just don't exercise the
	// fallback branch.
	id := resolveOrPersist("shell-runner", "/proc/croniq-runner-test-unwritable")
	if !strings.HasPrefix(id, "shell-runner-") {
		t.Errorf("fallback id missing prefix: %q", id)
	}
}

func TestResolveRunnerIDHonoursEnvOverride(t *testing.T) {
	t.Setenv("RUNNER_ID", "explicit-override-1")
	if got := ResolveRunnerID("ignored"); got != "explicit-override-1" {
		t.Errorf("env override not honoured; got %q", got)
	}
}
