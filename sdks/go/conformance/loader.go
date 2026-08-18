package conformance

import (
	"bytes"
	"errors"
	"fmt"
	"io"
	"os"

	"gopkg.in/yaml.v3"
)

// LoadFile reads a single case YAML and normalises the body trees so
// downstream JSON encoding round-trips cleanly. yaml.v3 already returns
// map[string]any for object nodes (unlike Go's older yaml.v2 which used
// map[any]any), so the normalisation is just a recursive walk that
// rejects anything we wouldn't be able to encode.
//
// Decoding is strict: KnownFields(true) makes a key that [Spec] does not
// model a load-time error instead of a silent drop. See [strictDecode].
func LoadFile(path string) (*Spec, error) {
	buf, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read case %s: %w", path, err)
	}
	var spec Spec
	if err := strictDecode(buf, &spec); err != nil {
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

// strictDecode unmarshals a case YAML with unknown-field rejection enabled.
//
// Why not plain yaml.Unmarshal: gopkg.in/yaml.v3 drops keys that the target
// struct does not model, so a case using an assertion key this binding has
// not implemented would load cleanly and then simply not be asserted — a
// green suite for an unenforced contract (#460). KnownFields(true) turns that
// silence into a parse error naming the offending key, which is the whole
// point: a schema addition must fail here until the binding implements it.
//
// The corpus is separately validated against schema/case-schema.json and
// schema/trigger-case-schema.json by CI, so the two checks are complementary
// rather than redundant — CI catches a key the *schema* does not allow, this
// catches a schema-legal key the *binding* does not implement.
func strictDecode(buf []byte, out any) error {
	dec := yaml.NewDecoder(bytes.NewReader(buf))
	dec.KnownFields(true)
	if err := dec.Decode(out); err != nil {
		if errors.Is(err, io.EOF) {
			return errors.New("empty YAML document")
		}
		return err
	}
	return nil
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
