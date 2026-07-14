//! Minimal producer example — fire a job on demand via `POST /v1/trigger`.
//!
//! ```sh
//! cargo run --example trigger
//! ```
//!
//! Unlike the runner (consumer) side, triggering requires the `jobs:trigger`
//! (or `admin`) scope, so the trigger client carries its own credentials.

use std::collections::HashMap;

use croniq_runner_sdk::{TriggerClient, TriggerError};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let client = TriggerClient::builder("http://localhost:4000")
        .api_key("croniq_your_trigger_key_here")
        .build();

    let result = client
        .trigger("billing:invoice")
        .metadata(HashMap::from([("invoice_id".into(), "inv_42".into())]))
        .idempotency_key("evt-2026-07-14-001")
        .send()
        .await;

    match result {
        Ok(res) => tracing::info!(
            execution_id = %res.execution_id,
            queued = res.queued,
            deduplicated = res.deduplicated,
            "job triggered",
        ),
        Err(TriggerError::QueueOverflow { .. }) => {
            // Per-job queue-overflow backpressure (issue #299): back off and
            // retry later rather than pushing the queue past its cap.
            tracing::warn!("job queue is full — backing off");
        }
        Err(e) => tracing::error!(error = %e, "trigger failed"),
    }
}
