//! CroniqRunner: the main orchestrator for job execution runners.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

use crate::client::{AckRequest, CroniqClient, PollRequest, RegisterJobRequest, RenewRequest};
use crate::handler::{ExecutionContext, HandlerError, HandlerRegistry};

/// A job schedule to register on the server at startup.
struct JobSchedule {
    job_key: String,
    schedule: String,
}

/// Builder for constructing a CroniqRunner.
pub struct RunnerBuilder {
    server_url: String,
    runner_id: String,
    api_key: Option<String>,
    capabilities: Vec<String>,
    max_inflight: u32,
}

impl RunnerBuilder {
    pub fn api_key(mut self, key: &str) -> Self {
        self.api_key = Some(key.to_string());
        self
    }

    pub fn capabilities(mut self, caps: Vec<String>) -> Self {
        self.capabilities = caps;
        self
    }

    pub fn max_inflight(mut self, n: u32) -> Self {
        self.max_inflight = n;
        self
    }

    pub fn build(self) -> CroniqRunner {
        let mut client = CroniqClient::new(&self.server_url);
        if let Some(key) = &self.api_key {
            client = client.with_api_key(key);
        }

        CroniqRunner {
            client: Arc::new(client),
            runner_id: self.runner_id,
            capabilities: self.capabilities,
            max_inflight: self.max_inflight,
            instance_id: uuid::Uuid::new_v4().to_string(),
            handlers: Arc::new(RwLock::new(HandlerRegistry::new())),
            schedules: Arc::new(RwLock::new(Vec::new())),
            inflight: Arc::new(RwLock::new(Vec::new())),
            draining: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

/// The main runner orchestrator.
pub struct CroniqRunner {
    client: Arc<CroniqClient>,
    runner_id: String,
    capabilities: Vec<String>,
    max_inflight: u32,
    instance_id: String,
    handlers: Arc<RwLock<HandlerRegistry>>,
    schedules: Arc<RwLock<Vec<JobSchedule>>>,
    inflight: Arc<RwLock<Vec<String>>>,
    draining: Arc<std::sync::atomic::AtomicBool>,
}

impl CroniqRunner {
    pub fn builder(server_url: &str, runner_id: &str) -> RunnerBuilder {
        RunnerBuilder {
            server_url: server_url.to_string(),
            runner_id: runner_id.to_string(),
            api_key: None,
            capabilities: Vec::new(),
            max_inflight: 5,
        }
    }

    /// Register a handler for a specific job key (job must already exist on server).
    pub async fn register<F, Fut>(&self, job_key: &str, handler: F)
    where
        F: Fn(ExecutionContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), HandlerError>> + Send + 'static,
    {
        self.handlers.write().await.register(job_key, handler);
    }

    /// Register a handler AND its schedule on the server.
    ///
    /// The server will create the job + trigger if they don't exist.
    /// If the job is already managed by the Croniqfile, the schedule is skipped
    /// (DSL has precedence).
    pub async fn register_with_schedule<F, Fut>(&self, job_key: &str, schedule: &str, handler: F)
    where
        F: Fn(ExecutionContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), HandlerError>> + Send + 'static,
    {
        self.handlers.write().await.register(job_key, handler);
        self.schedules.write().await.push(JobSchedule {
            job_key: job_key.to_string(),
            schedule: schedule.to_string(),
        });
    }

    /// Register a catch-all handler invoked when no specific handler matches the job key.
    pub async fn set_default_handler<F, Fut>(&self, handler: F)
    where
        F: Fn(ExecutionContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), HandlerError>> + Send + 'static,
    {
        self.handlers.write().await.set_default(handler);
    }

    /// Signal graceful shutdown: stop accepting new work, wait for inflight.
    pub fn drain(&self) {
        self.draining
            .store(true, std::sync::atomic::Ordering::Relaxed);
        tracing::info!(runner_id = %self.runner_id, "draining — no new work will be accepted");
    }

    /// Start the runner: auto-register jobs, then poll loop + lease renewal.
    pub async fn start(&self) -> Result<(), crate::client::ClientError> {
        tracing::info!(
            runner_id = %self.runner_id,
            capabilities = ?self.capabilities,
            max_inflight = self.max_inflight,
            "runner starting"
        );

        // Auto-register jobs with schedules on the server
        let schedules = self.schedules.read().await;
        for sched in schedules.iter() {
            tracing::info!(job_key = %sched.job_key, schedule = %sched.schedule, "registering job on server");
            match self
                .client
                .register_job(&RegisterJobRequest {
                    job_key: sched.job_key.clone(),
                    schedule: sched.schedule.clone(),
                    timezone: None,
                    timeout: None,
                    runner_id: Some(self.runner_id.clone()),
                    capabilities: self.capabilities.clone(),
                    description: None,
                })
                .await
            {
                Ok(()) => tracing::info!(job_key = %sched.job_key, "job registered"),
                Err(e) => {
                    tracing::warn!(job_key = %sched.job_key, error = %e, "failed to register job — will still poll")
                }
            }
        }
        drop(schedules);

        loop {
            if self.draining.load(std::sync::atomic::Ordering::Relaxed) {
                let inflight = self.inflight.read().await;
                if inflight.is_empty() {
                    tracing::info!("drain complete — all inflight work finished");
                    return Ok(());
                }
                tracing::debug!(inflight = inflight.len(), "draining — waiting for inflight");
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }

            let inflight = self.inflight.read().await.clone();
            let capacity = (self.max_inflight as usize).saturating_sub(inflight.len());

            if capacity == 0 {
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }

            let poll_req = PollRequest {
                runner_id: self.runner_id.clone(),
                capabilities: self.capabilities.clone(),
                max_inflight: self.max_inflight,
                inflight,
                instance_id: Some(self.instance_id.clone()),
            };

            match self.client.poll(&poll_req).await {
                Ok(resp) => {
                    for assignment in resp.work {
                        let exec_id = assignment.execution_id.clone();
                        let job_key = assignment.job_key.clone();
                        self.inflight.write().await.push(exec_id.clone());

                        let ctx = ExecutionContext::from(assignment);
                        let handlers = Arc::clone(&self.handlers);
                        let client = Arc::clone(&self.client);
                        let runner_id = self.runner_id.clone();
                        let inflight = Arc::clone(&self.inflight);

                        tokio::spawn(async move {
                            let attempt = ctx.attempt;

                            // Find handler
                            let handler = {
                                let reg = handlers.read().await;
                                reg.get(&job_key).cloned()
                            };

                            let (status, error, duration_ms) = if let Some(handler) = handler {
                                // Spawn lease renewal
                                let renew_client = Arc::clone(&client);
                                let renew_runner_id = runner_id.clone();
                                let renew_exec_id = exec_id.clone();
                                let renew_handle = tokio::spawn(async move {
                                    loop {
                                        tokio::time::sleep(Duration::from_secs(15)).await;
                                        let _ = renew_client
                                            .renew(&RenewRequest {
                                                runner_id: renew_runner_id.clone(),
                                                execution_id: renew_exec_id.clone(),
                                            })
                                            .await;
                                    }
                                });

                                let start = std::time::Instant::now();
                                let result = handler(ctx).await;
                                renew_handle.abort();

                                let duration_ms = start.elapsed().as_millis() as i64;
                                match result {
                                    Ok(()) => ("success".to_string(), None, duration_ms),
                                    Err(e) => {
                                        ("failure".to_string(), Some(e.to_string()), duration_ms)
                                    }
                                }
                            } else {
                                (
                                    "failure".to_string(),
                                    Some(format!("no handler registered for {job_key}")),
                                    0,
                                )
                            };

                            // Ack
                            let ack = AckRequest {
                                runner_id,
                                execution_id: exec_id.clone(),
                                status,
                                error,
                                duration_ms: Some(duration_ms),
                                attempt,
                            };
                            if let Err(e) = client.ack(&ack).await {
                                tracing::error!(execution_id = %exec_id, error = %e, "failed to ack");
                            }

                            // Remove from inflight
                            inflight.write().await.retain(|id| id != &exec_id);
                        });
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "poll failed — retrying in 5s");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
}
