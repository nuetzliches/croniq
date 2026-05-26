//! W3C TraceContext propagation: stamp the active span's `traceparent`
//! (and `tracestate`, when present) into the metadata that ships with a
//! `WorkAssignment` so the runner SDK can continue the same distributed
//! trace.
//!
//! Without the `otlp` feature the helper is a no-op and metadata is
//! untouched, matching the behaviour before this propagation existed.
//! With `otlp` compiled in but no `OTEL_EXPORTER_OTLP_ENDPOINT`
//! configured, the current span has no valid OTel context — injecting
//! `00-00…-00…-00` would be worse than no header at all, so the helper
//! still skips in that case.
//!
//! Wire format: top-level `traceparent` / `tracestate` string entries
//! on the metadata JSON object, matching the W3C HTTP-header convention
//! exactly so SDK consumers can hand the map straight to their
//! platform's `TextMapPropagator::extract` (or `Propagators.DefaultTextMapPropagator.Extract`
//! in .NET, `opentelemetry.propagate` in Python, …).

/// Inject the active span's W3C TraceContext into the given metadata
/// object. Silently does nothing if the metadata is not a JSON object,
/// the `otlp` feature is off, or there is no valid span context.
#[cfg(feature = "otlp")]
pub fn inject_into_metadata(metadata: &mut serde_json::Value) {
    use opentelemetry::propagation::TextMapPropagator;
    use opentelemetry::trace::TraceContextExt;
    use opentelemetry_sdk::propagation::TraceContextPropagator;
    use std::collections::HashMap;
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    let Some(obj) = metadata.as_object_mut() else {
        // Defensive — callers always build a JSON object; if that ever
        // regresses we'd rather skip than panic on a metadata payload
        // mid-fire.
        return;
    };

    let cx = tracing::Span::current().context();
    if !cx.span().span_context().is_valid() {
        // Either there's no tracer provider installed (the `otlp`
        // feature is compiled in but `OTEL_EXPORTER_OTLP_ENDPOINT` is
        // unset) or this code runs outside any instrumented span. In
        // both cases the injected traceparent would be all zeros,
        // which conveys nothing and risks confusing downstream
        // collectors.
        return;
    }

    let mut carrier: HashMap<String, String> = HashMap::new();
    TraceContextPropagator::new().inject_context(&cx, &mut carrier);

    for (k, v) in carrier {
        obj.insert(k, serde_json::Value::String(v));
    }
}

/// No-op when the OTLP feature is off: without `tracing-opentelemetry`
/// in the build, there is no OTel context attached to tracing spans,
/// so there is nothing meaningful to inject.
#[cfg(not(feature = "otlp"))]
pub fn inject_into_metadata(_metadata: &mut serde_json::Value) {}

#[cfg(all(test, feature = "otlp"))]
mod tests {
    use super::*;
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use tracing_subscriber::layer::SubscriberExt;

    /// Install a thread-local subscriber wired with the
    /// `tracing-opentelemetry` bridge so `Span::current().context()`
    /// inside the helper resolves to a real OTel span context. Each
    /// test gets its own subscriber → parallel-safe, no global state.
    #[test]
    fn injects_w3c_traceparent_when_span_is_active() {
        let provider = SdkTracerProvider::builder().build();
        let tracer = provider.tracer("test");
        let subscriber =
            tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));

        let mut metadata = serde_json::json!({ "user_field": "kept" });

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("fire_job");
            span.in_scope(|| {
                inject_into_metadata(&mut metadata);
            });
        });

        let obj = metadata.as_object().expect("metadata stays an object");
        let tp = obj
            .get("traceparent")
            .and_then(|v| v.as_str())
            .expect("traceparent must be stamped when a valid span is active");

        // W3C version-00 traceparent: `00-<32 hex trace-id>-<16 hex span-id>-<2 hex flags>`.
        let parts: Vec<&str> = tp.split('-').collect();
        assert_eq!(parts.len(), 4, "traceparent has 4 hyphen-delimited fields");
        assert_eq!(parts[0], "00", "version is 00");
        assert_eq!(parts[1].len(), 32, "trace-id is 32 hex chars");
        assert_eq!(parts[2].len(), 16, "span-id is 16 hex chars");
        assert_eq!(parts[3].len(), 2, "flags is 2 hex chars");

        // User-supplied metadata survives the merge.
        assert_eq!(obj.get("user_field").and_then(|v| v.as_str()), Some("kept"));
    }

    #[test]
    fn noop_when_no_valid_span_in_scope() {
        // No tracer provider, no subscriber — `Span::current()` is the
        // disabled root and `span_context().is_valid()` is false.
        // Helper must leave metadata byte-identical.
        let before = serde_json::json!({ "month": "2026-05" });
        let mut after = before.clone();
        inject_into_metadata(&mut after);
        assert_eq!(before, after);
    }

    #[test]
    fn noop_when_metadata_is_not_an_object() {
        let mut metadata = serde_json::Value::String("oops".to_string());
        inject_into_metadata(&mut metadata);
        assert_eq!(metadata, serde_json::Value::String("oops".to_string()));
    }
}
