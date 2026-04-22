//! Catch-all runner example — handles any job the server sends.
//!
//! Useful when a single runner acts as a fallback / generic worker for many
//! job keys without having to register a handler per key.
//!
//! ```sh
//! cargo run --example catch_all
//! ```

use croniq_runner_sdk::{CroniqRunner, ExecutionContext, HandlerError};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let runner = CroniqRunner::builder("http://localhost:4000", "catch-all")
        .api_key("croniq_your_key_here")
        .max_inflight(5)
        .build();

    // `set_default_handler` is invoked whenever no specific handler matches
    // the incoming job key. Specific handlers registered with `register` or
    // `register_with_schedule` still take precedence when they match.
    runner
        .set_default_handler(|ctx: ExecutionContext| async move {
            tracing::info!(
                job_key = %ctx.job_key,
                execution_id = %ctx.execution_id,
                "handling unknown job"
            );
            // ... dispatch to your own logic based on ctx.job_key / ctx.metadata ...
            if ctx.job_key.starts_with("report:") {
                return Err(HandlerError::msg("reports are handled by another runner"));
            }
            Ok(())
        })
        .await;

    runner.start().await.unwrap();
}
