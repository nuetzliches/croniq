package croniq

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
)

// Resolution order for [ResolveRunnerID].
const (
	defaultDataDir = "/var/lib/croniq-runner"
	stateFileName  = "runner-id"
)

// ResolveRunnerID returns a stable runner_id across container recreates.
//
// Resolution order:
//
//  1. RUNNER_ID env var — explicit operator override, used as-is.
//  2. State file at ${CRONIQ_RUNNER_DATA_DIR:-/var/lib/croniq-runner}/runner-id
//     — read if it exists and non-empty.
//  3. Generate "{prefix}-{8-hex}" and persist to the state file.
//
// If persistence fails (no volume mounted, read-only filesystem, …),
// falls back to a volatile hostname-derived id and logs a warning. The
// runner still starts; only the cross-recreate stability is lost.
func ResolveRunnerID(prefix string) string {
	if v := strings.TrimSpace(os.Getenv("RUNNER_ID")); v != "" {
		return v
	}
	dir := strings.TrimSpace(os.Getenv("CRONIQ_RUNNER_DATA_DIR"))
	if dir == "" {
		dir = defaultDataDir
	}
	return resolveOrPersist(prefix, dir)
}

func resolveOrPersist(prefix, dataDir string) string {
	statePath := filepath.Join(dataDir, stateFileName)

	if buf, err := os.ReadFile(statePath); err == nil {
		id := strings.TrimSpace(string(buf))
		if id != "" {
			return id
		}
	}

	id := generateID(prefix)
	if err := persistID(statePath, id); err != nil {
		fallback := volatileFallback(prefix)
		slog.Warn(
			"could not persist runner identity — falling back to volatile ID. Mount a writable volume at this path to make runner_id stable across container recreates.",
			"path", statePath,
			"error", err,
			"runner_id", fallback,
		)
		return fallback
	}
	slog.Info("generated new runner identity and persisted to state file",
		"path", statePath,
		"runner_id", id,
	)
	return id
}

func generateID(prefix string) string {
	var buf [4]byte
	if _, err := rand.Read(buf[:]); err != nil {
		// crypto/rand failure is "the OS is broken" territory — fall
		// back to something rather than panicking, the runner can
		// still start.
		return fmt.Sprintf("%s-%08x", prefix, os.Getpid())
	}
	return fmt.Sprintf("%s-%s", prefix, hex.EncodeToString(buf[:]))
}

func persistID(path, id string) error {
	if path == "" {
		return errors.New("empty state-file path")
	}
	if dir := filepath.Dir(path); dir != "" {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return err
		}
	}
	return os.WriteFile(path, []byte(id), 0o644)
}

func volatileFallback(prefix string) string {
	if host := strings.TrimSpace(os.Getenv("HOSTNAME")); host != "" {
		return prefix + "-" + host
	}
	return generateID(prefix)
}
