package croniq

import "encoding/json"

// serializeTags JSON-encodes the runner's tag list for inclusion as a
// single `runner_tags` log field. Returns "" when the runner has no tags.
func serializeTags(tags []string) string {
	if len(tags) == 0 {
		return ""
	}
	buf, err := json.Marshal(tags)
	if err != nil {
		return ""
	}
	return string(buf)
}

// enrichEvent auto-injects `job_key`, `runner_id`, and `runner_tags`
// into a log event's fields without overwriting caller-provided values.
// Returns a new event; the input is not mutated.
func enrichEvent(in WorkEvent, jobKey, runnerID, serializedTags string) WorkEvent {
	fields := make(map[string]string, len(in.Fields)+3)
	for k, v := range in.Fields {
		fields[k] = v
	}
	if _, ok := fields["job_key"]; !ok {
		fields["job_key"] = jobKey
	}
	if _, ok := fields["runner_id"]; !ok {
		fields["runner_id"] = runnerID
	}
	if serializedTags != "" {
		if _, ok := fields["runner_tags"]; !ok {
			fields["runner_tags"] = serializedTags
		}
	}
	return WorkEvent{
		Level:   in.Level,
		Message: in.Message,
		Fields:  fields,
	}
}
