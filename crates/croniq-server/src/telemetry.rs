//! Telemetry initialisation: stderr logs by default, optional OTLP exporter
//! for traces + logs when the `otlp` feature is enabled and the standard
//! `OTEL_EXPORTER_OTLP_ENDPOINT` env var is set.
//!
//! Behaviour matrix:
//!
//! | feature `otlp` | `OTEL_EXPORTER_OTLP_ENDPOINT` | Effect                                 |
//! |----------------|-------------------------------|----------------------------------------|
//! | off            | (any)                         | stderr `fmt` only (today's behaviour)  |
//! | on             | unset / blank                 | stderr `fmt` only                      |
//! | on             | set                           | stderr `fmt` + OTLP spans + OTLP logs  |
//!
//! Protocol selection (`OTEL_EXPORTER_OTLP_PROTOCOL`) and service identity
//! (`OTEL_SERVICE_NAME`, `OTEL_RESOURCE_ATTRIBUTES`) are read by the
//! `opentelemetry-otlp` / `opentelemetry_sdk` builders directly — we don't
//! re-parse them. The `decide()` function below mirrors the endpoint check
//! so it can be unit-tested without touching the global subscriber.

use std::collections::HashMap;

use anyhow::Result;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Telemetry mode derived from environment variables.
///
/// Split out as a pure function so we can unit-test the endpoint-driven
/// install decision without touching the global subscriber. Tests
/// construct an [`Env`] from a `HashMap` rather than mutating
/// `std::env`, which keeps them parallel-safe.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TelemetryMode {
    /// stderr `tracing-subscriber::fmt` only. Either the `otlp` feature is
    /// off, or the OTLP endpoint env var is unset/blank.
    StderrOnly,
    /// stderr + OTLP. The actual endpoint string is consumed by the SDK
    /// builders directly; we only carry it for diagnostic logging.
    #[cfg_attr(not(feature = "otlp"), allow(dead_code))]
    WithOtlp { endpoint: String },
}

/// Abstracted env read for testability.
#[derive(Debug, Default)]
pub(crate) struct Env {
    vars: HashMap<String, String>,
}

impl Env {
    pub(crate) fn from_process() -> Self {
        Self {
            vars: std::env::vars().collect(),
        }
    }

    #[cfg(test)]
    fn with(mut self, key: &str, value: &str) -> Self {
        self.vars.insert(key.to_string(), value.to_string());
        self
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }
}

/// Decide which telemetry pipeline to install. Pure function — no side
/// effects, no global state — so tests can exercise every branch.
pub(crate) fn decide(env: &Env) -> TelemetryMode {
    // With the feature off, OTLP env vars are intentionally ignored: the
    // user did not opt in at compile time, so honouring runtime env would
    // be misleading (it would silently do nothing). Match the issue spec
    // exactly.
    if !cfg!(feature = "otlp") {
        return TelemetryMode::StderrOnly;
    }

    match env.get("OTEL_EXPORTER_OTLP_ENDPOINT") {
        Some(endpoint) if !endpoint.trim().is_empty() => TelemetryMode::WithOtlp {
            endpoint: endpoint.to_string(),
        },
        _ => TelemetryMode::StderrOnly,
    }
}

/// Holds OTLP providers so they can be flushed on shutdown. With the
/// `otlp` feature off, this is effectively a unit-sized type and
/// `shutdown` is a no-op.
pub struct TelemetryGuard {
    #[cfg(feature = "otlp")]
    tracer_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
    #[cfg(feature = "otlp")]
    logger_provider: Option<opentelemetry_sdk::logs::SdkLoggerProvider>,
}

impl TelemetryGuard {
    fn empty() -> Self {
        Self {
            #[cfg(feature = "otlp")]
            tracer_provider: None,
            #[cfg(feature = "otlp")]
            logger_provider: None,
        }
    }

    /// Flush in-flight spans / log records and tear down the exporters.
    /// Safe to call multiple times; only the first call has effect.
    #[cfg_attr(not(feature = "otlp"), allow(unused_mut))]
    pub fn shutdown(mut self) {
        #[cfg(feature = "otlp")]
        {
            if let Some(p) = self.tracer_provider.take()
                && let Err(e) = p.shutdown()
            {
                eprintln!("croniq: OTLP tracer shutdown error: {e}");
            }
            if let Some(p) = self.logger_provider.take()
                && let Err(e) = p.shutdown()
            {
                eprintln!("croniq: OTLP logger shutdown error: {e}");
            }
        }
    }
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        #[cfg(feature = "otlp")]
        {
            if let Some(p) = self.tracer_provider.take() {
                let _ = p.shutdown();
            }
            if let Some(p) = self.logger_provider.take() {
                let _ = p.shutdown();
            }
        }
    }
}

/// Initialise the global tracing subscriber. Always installs the stderr
/// `fmt` layer (matching pre-OTLP behaviour). When the `otlp` feature is
/// compiled in and `OTEL_EXPORTER_OTLP_ENDPOINT` is set, also installs
/// OTLP span + log layers in parallel.
///
/// Returns a guard whose [`TelemetryGuard::shutdown`] should be called
/// before the process exits, so the OTLP batch exporters can flush.
pub fn init() -> Result<TelemetryGuard> {
    let env = Env::from_process();
    let mode = decide(&env);

    match mode {
        TelemetryMode::StderrOnly => {
            let env_filter =
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer().with_writer(std::io::stderr))
                .init();
            Ok(TelemetryGuard::empty())
        }
        #[cfg_attr(not(feature = "otlp"), allow(unused_variables))]
        TelemetryMode::WithOtlp { endpoint } => {
            #[cfg(not(feature = "otlp"))]
            {
                unreachable!("decide() returns StderrOnly without the otlp feature")
            }
            #[cfg(feature = "otlp")]
            {
                init_otlp(endpoint)
            }
        }
    }
}

#[cfg(feature = "otlp")]
fn init_otlp(endpoint: String) -> Result<TelemetryGuard> {
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
    use opentelemetry_sdk::Resource;
    use opentelemetry_sdk::logs::SdkLoggerProvider;
    use opentelemetry_sdk::trace::SdkTracerProvider;

    // Resource: SDK reads OTEL_SERVICE_NAME / OTEL_RESOURCE_ATTRIBUTES on
    // its own; "croniq" is the fallback when both are unset.
    let resource = Resource::builder().with_service_name("croniq").build();

    // Both span + log exporters use `.build()` without an explicit
    // transport selector, so opentelemetry-otlp picks the transport from
    // `OTEL_EXPORTER_OTLP_PROTOCOL` (gRPC default; `http/protobuf` or
    // `http/json` to switch). Both `grpc-tonic` and `http-proto` are
    // compiled in via the `otlp` feature so either choice works without
    // recompiling.
    let span_exporter = opentelemetry_otlp::SpanExporter::builder().build()?;
    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(span_exporter)
        .with_resource(resource.clone())
        .build();

    let log_exporter = opentelemetry_otlp::LogExporter::builder().build()?;
    let logger_provider = SdkLoggerProvider::builder()
        .with_batch_exporter(log_exporter)
        .with_resource(resource)
        .build();

    let tracer = tracer_provider.tracer("croniq");
    let otel_span_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let otel_log_layer = OpenTelemetryTracingBridge::new(&logger_provider);

    // Separate filter for the OTLP log bridge: if the user sets
    // `RUST_LOG=trace` we don't want to flood the collector with
    // library-internal events. Default to INFO+ for OTLP, override via
    // OTEL_LOG_LEVEL if the operator wants finer-grained control.
    let otlp_filter = std::env::var("OTEL_LOG_LEVEL")
        .ok()
        .and_then(|v| EnvFilter::try_new(v).ok())
        .unwrap_or_else(|| EnvFilter::new("info"));
    let otel_log_layer = otel_log_layer.with_filter(otlp_filter);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(otel_span_layer)
        .with(otel_log_layer)
        .init();

    tracing::info!(
        endpoint = %endpoint,
        protocol = %std::env::var("OTEL_EXPORTER_OTLP_PROTOCOL").unwrap_or_else(|_| "grpc".to_string()),
        "OTLP exporter installed"
    );

    Ok(TelemetryGuard {
        tracer_provider: Some(tracer_provider),
        logger_provider: Some(logger_provider),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_stderr_only_when_endpoint_unset() {
        let env = Env::default();
        assert_eq!(decide(&env), TelemetryMode::StderrOnly);
    }

    #[test]
    fn decide_stderr_only_when_endpoint_blank() {
        // Common docker-compose gotcha: an empty env var still gets
        // exported, but blank means "no endpoint configured".
        let env = Env::default().with("OTEL_EXPORTER_OTLP_ENDPOINT", "   ");
        assert_eq!(decide(&env), TelemetryMode::StderrOnly);
    }

    #[cfg(feature = "otlp")]
    #[test]
    fn decide_otlp_when_endpoint_set() {
        let env = Env::default().with("OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4317");
        assert_eq!(
            decide(&env),
            TelemetryMode::WithOtlp {
                endpoint: "http://collector:4317".to_string(),
            }
        );
    }

    #[cfg(not(feature = "otlp"))]
    #[test]
    fn decide_stderr_only_when_feature_off_even_if_endpoint_set() {
        // Without the otlp feature, OTLP env vars are intentionally
        // ignored — the user did not opt in at compile time.
        let env = Env::default().with("OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4317");
        assert_eq!(decide(&env), TelemetryMode::StderrOnly);
    }
}
