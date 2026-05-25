package otel_test

import (
	"context"
	"errors"
	"testing"

	"go.opentelemetry.io/otel/attribute"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	"go.opentelemetry.io/otel/sdk/trace/tracetest"
	"go.opentelemetry.io/otel/trace"

	croniq "github.com/nuetzliches/croniq/sdks/go"
	croniqotel "github.com/nuetzliches/croniq/sdks/go/otel"
)

func newTracerWithRecorder() (trace.Tracer, *tracetest.SpanRecorder) {
	rec := tracetest.NewSpanRecorder()
	tp := sdktrace.NewTracerProvider(sdktrace.WithSpanProcessor(rec))
	return tp.Tracer("croniq-test"), rec
}

func TestTracingMiddlewareWrapsSuccessfulHandler(t *testing.T) {
	tracer, rec := newTracerWithRecorder()

	mw := croniqotel.TracingMiddleware(tracer)
	wrapped := mw(func(_ context.Context, _ *croniq.ExecutionContext) error {
		return nil
	})

	ec := &croniq.ExecutionContext{
		ExecutionID: "exec-1",
		JobKey:      "billing:invoice",
		Attempt:     1,
		RunnerID:    "runner-1",
	}
	if err := wrapped(context.Background(), ec); err != nil {
		t.Fatalf("wrapped handler errored: %v", err)
	}

	spans := rec.Ended()
	if len(spans) != 1 {
		t.Fatalf("expected 1 span, got %d", len(spans))
	}
	if got := spans[0].Name(); got != "croniq.execute billing:invoice" {
		t.Errorf("span name = %q", got)
	}

	attrs := attrMap(spans[0].Attributes())
	if got := attrs[croniqotel.AttrJobKey]; got != "billing:invoice" {
		t.Errorf("job.key = %q", got)
	}
	if got := attrs[croniqotel.AttrExecutionID]; got != "exec-1" {
		t.Errorf("execution.id = %q", got)
	}
	if got := attrs[croniqotel.AttrRunnerID]; got != "runner-1" {
		t.Errorf("runner.id = %q", got)
	}
	if got := attrs[croniqotel.AttrExecutionOutcome]; got != "success" {
		t.Errorf("outcome = %q, want success", got)
	}
}

func TestTracingMiddlewareRecordsErrorOnFailure(t *testing.T) {
	tracer, rec := newTracerWithRecorder()

	mw := croniqotel.TracingMiddleware(tracer)
	wrapped := mw(func(_ context.Context, _ *croniq.ExecutionContext) error {
		return errors.New("boom")
	})

	ec := &croniq.ExecutionContext{
		ExecutionID: "exec-2",
		JobKey:      "broken:job",
		Attempt:     3,
		RunnerID:    "runner-1",
	}
	err := wrapped(context.Background(), ec)
	if err == nil || err.Error() != "boom" {
		t.Fatalf("wrapped err = %v", err)
	}

	spans := rec.Ended()
	if len(spans) != 1 {
		t.Fatalf("expected 1 span, got %d", len(spans))
	}
	attrs := attrMap(spans[0].Attributes())
	if got := attrs[croniqotel.AttrExecutionOutcome]; got != "failure" {
		t.Errorf("outcome = %q, want failure", got)
	}
	events := spans[0].Events()
	if len(events) == 0 {
		t.Errorf("expected at least one event (RecordError) on failure span")
	}
}

func attrMap(kvs []attribute.KeyValue) map[string]string {
	out := make(map[string]string, len(kvs))
	for _, kv := range kvs {
		out[string(kv.Key)] = kv.Value.Emit()
	}
	return out
}
