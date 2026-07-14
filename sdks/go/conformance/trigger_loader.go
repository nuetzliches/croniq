package conformance

import (
	"fmt"
	"os"

	"gopkg.in/yaml.v3"
)

// LoadTriggerFile reads a single trigger (producer) case YAML and normalises
// the JSON body trees — request metadata, server_script response bodies, and
// body_match expectations — so downstream JSON encoding and subset matching
// round-trip cleanly. Mirrors [LoadFile] for runner cases.
func LoadTriggerFile(path string) (*TriggerSpec, error) {
	buf, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read trigger case %s: %w", path, err)
	}
	var spec TriggerSpec
	if err := yaml.Unmarshal(buf, &spec); err != nil {
		return nil, fmt.Errorf("parse trigger case %s: %w", path, err)
	}

	for i := range spec.TriggerCalls {
		if m := spec.TriggerCalls[i].Request.Metadata; m != nil {
			// normalise returns map[string]any for a map[string]any input.
			spec.TriggerCalls[i].Request.Metadata, _ = normalise(m).(map[string]any)
		}
	}
	for i := range spec.ServerScript {
		spec.ServerScript[i].Respond.Body = normalise(spec.ServerScript[i].Respond.Body)
	}
	for i := range spec.Expectations.HTTP {
		spec.Expectations.HTTP[i].BodyMatch = normalise(spec.Expectations.HTTP[i].BodyMatch)
	}
	return &spec, nil
}
