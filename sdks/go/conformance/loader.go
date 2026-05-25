package conformance

import (
	"fmt"
	"os"

	"gopkg.in/yaml.v3"
)

// LoadFile reads a single case YAML and normalises the body trees so
// downstream JSON encoding round-trips cleanly. yaml.v3 already returns
// map[string]any for object nodes (unlike Go's older yaml.v2 which used
// map[any]any), so the normalisation is just a recursive walk that
// rejects anything we wouldn't be able to encode.
func LoadFile(path string) (*Spec, error) {
	buf, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read case %s: %w", path, err)
	}
	var spec Spec
	if err := yaml.Unmarshal(buf, &spec); err != nil {
		return nil, fmt.Errorf("parse case %s: %w", path, err)
	}

	for i := range spec.ServerScript {
		spec.ServerScript[i].Respond.Body = normalise(spec.ServerScript[i].Respond.Body)
	}
	for i := range spec.Expectations.HTTP {
		spec.Expectations.HTTP[i].BodyMatch = normalise(spec.Expectations.HTTP[i].BodyMatch)
	}
	return &spec, nil
}

// normalise walks a yaml-decoded value and converts any
// map[any]any nodes (which yaml.v3 only produces for non-string keys —
// rare but possible) into map[string]any, since the rest of the harness
// (JSON encoder, body matcher) only handles string-keyed maps.
func normalise(v any) any {
	switch x := v.(type) {
	case map[string]any:
		out := make(map[string]any, len(x))
		for k, vv := range x {
			out[k] = normalise(vv)
		}
		return out
	case map[any]any:
		out := make(map[string]any, len(x))
		for k, vv := range x {
			out[fmt.Sprintf("%v", k)] = normalise(vv)
		}
		return out
	case []any:
		out := make([]any, len(x))
		for i, vv := range x {
			out[i] = normalise(vv)
		}
		return out
	default:
		return v
	}
}
