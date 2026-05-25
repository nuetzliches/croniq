// Package otel is the opt-in OpenTelemetry adapter for the Croniq Go
// SDK. It lives in its own Go module so the core SDK stays
// dependency-light — users who don't run observability never pay the
// cost of pulling in `go.opentelemetry.io/otel`.
//
// # Quick start
//
//	import (
//	    "go.opentelemetry.io/otel"
//	    croniq "github.com/nuetzliches/croniq/sdks/go"
//	    croniqotel "github.com/nuetzliches/croniq/sdks/go/otel"
//	)
//
//	tracer := otel.Tracer("my-service")
//
//	r := croniq.NewRunner(serverURL, runnerID,
//	    croniq.WithMiddleware(croniqotel.TracingMiddleware(tracer)),
//	)
//
// Spans are named `croniq.execute {job_key}` and carry these attributes:
//
//   - croniq.job.key — the assignment's job_key
//   - croniq.execution.id — the assignment's execution_id
//   - croniq.execution.attempt — current attempt number
//   - croniq.runner.id — the runner_id that picked it up
//   - croniq.execution.outcome — "success" or "failure"
package otel

import (
	"context"

	croniq "github.com/nuetzliches/croniq/sdks/go"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/trace"
)

// Standard attribute keys.
const (
	AttrJobKey           = "croniq.job.key"
	AttrExecutionID      = "croniq.execution.id"
	AttrExecutionAttempt = "croniq.execution.attempt"
	AttrRunnerID         = "croniq.runner.id"
	AttrExecutionOutcome = "croniq.execution.outcome"
)

// TracingMiddleware wraps every handler in a span named
// `croniq.execute {job_key}` and attaches the standard attributes.
// Pass any `trace.Tracer` — typically `otel.Tracer("your-service")`.
//
// The middleware is a thin shim — your runner owns the OTLP exporter
// and resource setup; this package does not impose either.
func TracingMiddleware(tracer trace.Tracer) croniq.Middleware {
	return func(next croniq.HandlerFunc) croniq.HandlerFunc {
		return func(ctx context.Context, ec *croniq.ExecutionContext) error {
			ctx, span := tracer.Start(ctx, "croniq.execute "+ec.JobKey,
				trace.WithSpanKind(trace.SpanKindConsumer),
				trace.WithAttributes(
					attribute.String(AttrJobKey, ec.JobKey),
					attribute.String(AttrExecutionID, ec.ExecutionID),
					attribute.Int(AttrExecutionAttempt, ec.Attempt),
					attribute.String(AttrRunnerID, ec.RunnerID),
				),
			)
			defer span.End()

			err := next(ctx, ec)
			if err != nil {
				span.RecordError(err)
				span.SetAttributes(attribute.String(AttrExecutionOutcome, "failure"))
				span.SetStatus(codes.Error, err.Error())
				return err
			}
			span.SetAttributes(attribute.String(AttrExecutionOutcome, "success"))
			return nil
		}
	}
}
