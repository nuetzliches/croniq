//! Minimal runner example.
//!
//! ```sh
//! cargo run --example basic
//! ```

use croniq_runner_sdk::{CroniqRunner, ExecutionContext};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let runner = CroniqRunner::builder("http://localhost:4000", "example-runner")
        .api_key("croniq_your_key_here")
        .capabilities(vec!["billing".into()])
        .max_inflight(3)
        .build();

    // Register with schedule — the server creates the job + trigger automatically
    runner
        .register_with_schedule(
            "billing:invoice",
            "5m", // every 5 minutes
            |ctx: ExecutionContext| async move {
                tracing::info!(
                    execution_id = %ctx.execution_id,
                    attempt = ctx.attempt,
                    "processing invoice"
                );
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                tracing::info!("invoice processed successfully");
                Ok(())
            },
        )
        .await;

    tracing::info!("runner starting — press Ctrl+C to stop");

    tokio::select! {
        result = runner.start() => {
            if let Err(e) = result {
                tracing::error!(error = %e, "runner exited with error");
            }
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("shutting down...");
            runner.drain();
        }
    }
}
