package croniqrunner

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"sync"
)

type outboxEntry struct {
	ID       string          `json:"id"`
	Type     string          `json:"type"`
	Payload  json.RawMessage `json:"payload"`
	Attempts int             `json:"attempts"`
}

type outboxStore struct {
	path       string
	maxEntries int
	maxBytes   int64
	mu         sync.Mutex
	entries    []outboxEntry
}

func newOutboxStore(path string, maxEntries int, maxBytes int64) *outboxStore {
	return &outboxStore{path: path, maxEntries: maxEntries, maxBytes: maxBytes}
}

func (o *outboxStore) Load() {
	o.mu.Lock()
	defer o.mu.Unlock()

	data, err := os.ReadFile(o.path)
	if err != nil {
		o.entries = nil
		return
	}

	lines := strings.Split(string(data), "\n")
	entries := make([]outboxEntry, 0, len(lines))
	for _, line := range lines {
		trimmed := strings.TrimSpace(line)
		if trimmed == "" {
			continue
		}
		var entry outboxEntry
		if err := json.Unmarshal([]byte(trimmed), &entry); err == nil {
			entries = append(entries, entry)
		}
	}
	if len(entries) > o.maxEntries {
		entries = entries[len(entries)-o.maxEntries:]
	}
	outboxEntries := entries
	o.entries = outboxEntries
}

func (o *outboxStore) Items() []outboxEntry {
	o.mu.Lock()
	defer o.mu.Unlock()

	items := make([]outboxEntry, len(o.entries))
	copy(items, o.entries)
	return items
}

func (o *outboxStore) Enqueue(entry outboxEntry) {
	o.mu.Lock()
	o.entries = append(o.entries, entry)
	if len(o.entries) > o.maxEntries {
		o.entries = o.entries[len(o.entries)-o.maxEntries:]
	}
	o.mu.Unlock()
	o.persist()
}

func (o *outboxStore) Remove(entryID string) {
	o.mu.Lock()
	entries := o.entries[:0]
	for _, entry := range o.entries {
		if entry.ID != entryID {
			entries = append(entries, entry)
		}
	}
	o.entries = entries
	o.mu.Unlock()
	o.persist()
}

func (o *outboxStore) MarkFailed(entryID string) {
	o.mu.Lock()
	for i := range o.entries {
		if o.entries[i].ID == entryID {
			o.entries[i].Attempts++
			break
		}
	}
	o.mu.Unlock()
	o.persist()
}

func (o *outboxStore) persist() {
	o.mu.Lock()
	defer o.mu.Unlock()

	if err := os.MkdirAll(filepath.Dir(o.path), 0o755); err != nil {
		return
	}

	lines := make([]string, 0, len(o.entries))
	for _, entry := range o.entries {
		payload, err := json.Marshal(entry)
		if err == nil {
			lines = append(lines, string(payload))
		}
	}
	_ = os.WriteFile(o.path, []byte(strings.Join(lines, "\n")), 0o644)

	if info, err := os.Stat(o.path); err == nil && info.Size() > o.maxBytes {
		if len(o.entries) > 1 {
			o.entries = o.entries[len(o.entries)/2:]
			lines = lines[len(lines)/2:]
			_ = os.WriteFile(o.path, []byte(strings.Join(lines, "\n")), 0o644)
		}
	}
}
