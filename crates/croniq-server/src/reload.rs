//! Croniqfile reload helper.
//!
//! Builds a validated `ReloadPlan` from a path: parses the file, merges the
//! resulting DSL jobs with API-registered triggers from the store (DSL
//! precedence), and diffs the merged state against what the scheduler is
//! currently running. Does NOT mutate any state — callers decide whether to
//! apply the plan or discard it (dry-run).
//!
//! Used by:
//! - The file-watcher reload path
//! - The SIGHUP handler (unix only)
//! - `POST /v1/admin/reload-config`

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
use croniq_config::compile::{CalendarConfig, JobConfig};
use croniq_scheduler::trigger::Trigger;
use tokio::sync::{RwLock, mpsc, oneshot};

use crate::loader::{
    LoadError, LoadedConfig, job_config_from_definition, load_file, trigger_from_definition,
};
use crate::scheduler::{SchedulerCommand, SchedulerLoop};
use crate::store::DynStore;

/// Summary of how a reload will (or would) affect the scheduler state.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ReloadDiff {
    /// Job keys that will be added (present after reload, not before).
    pub added: Vec<String>,
    /// Job keys that will be removed (present before reload, not after).
    pub removed: Vec<String>,
    /// Job keys that exist in both, but with changed scheduling-relevant config.
    pub changed: Vec<String>,
    /// Total job count after the reload would apply.
    pub total: usize,
}

impl ReloadDiff {
    pub fn is_noop(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

// ─── Boot-only settings (issue #406) ─────────────────────────────────────────

/// A boot-only setting whose value in the Croniqfile differs from the value the
/// server is running with — parsed by the reload, then discarded.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PendingRestart {
    /// Dotted DSL path, e.g. `server.execution_retention`.
    pub setting: &'static str,
    /// What the running server uses. `None` ⇒ not set at boot.
    pub running: Option<String>,
    /// What the reloaded file asks for. `None` ⇒ removed from the file.
    pub pending: Option<String>,
}

/// Snapshot of every setting a reload parses but cannot apply.
///
/// A reload re-reads the whole file but only diffs and applies jobs, calendars,
/// triggers and the `policy { }` flags; `server { }`, `pull_api { }` and the
/// rest are dropped on the floor. That is documented ("take effect on server
/// restart"), but the reload still *looks* like a full apply, and with `--watch`
/// there was no signal at all that part of an edit had not landed — a trap in
/// container deployments, where a config-only change doesn't recreate the
/// container and so never restarts the process (issue #406).
///
/// Comparing snapshots is deliberately dumb: flatten to `path → rendered value`
/// and diff the maps, so a new boot-only directive only has to be added to
/// [`BootOnlySettings::from_runtime`] to be covered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BootOnlySettings(std::collections::BTreeMap<&'static str, String>);

impl BootOnlySettings {
    /// Flatten the boot-only settings of a compiled config. Settings that are
    /// unset are absent from the map rather than stored as an empty string, so
    /// "removed from the file" and "set to empty" stay distinguishable.
    pub fn from_runtime(rt: &croniq_config::compile::RuntimeConfig) -> Self {
        let mut m = std::collections::BTreeMap::new();
        let mut put = |k: &'static str, v: Option<String>| {
            if let Some(v) = v {
                m.insert(k, v);
            }
        };

        put("server.listen", Some(rt.server.listen.clone()));
        put("server.data_dir", Some(rt.server.data_dir.clone()));
        put("server.db", Some(rt.server.db.clone()));
        put("server.app_url", rt.server.app_url.clone());
        put(
            "server.execution_retention",
            rt.server.execution_retention.clone(),
        );

        if let Some(p) = rt.pull_api.as_ref() {
            put("pull_api.listen", Some(p.listen.clone()));
            put("pull_api.lease_ttl", Some(p.lease_ttl.clone()));
            put(
                "pull_api.trigger_dedup_window",
                Some(p.trigger_dedup_window.clone()),
            );
            put(
                "pull_api.runner_identity_binding",
                Some(p.runner_identity_binding.clone()),
            );
        }

        if let Some(o) = rt.observability.as_ref() {
            if let Some(l) = o.log.as_ref() {
                put("observability.log.level", Some(l.level.clone()));
                put("observability.log.format", Some(l.format.clone()));
                put("observability.log.output", Some(l.output.clone()));
            }
            if let Some(me) = o.metrics.as_ref() {
                put("observability.metrics.listen", Some(me.listen.clone()));
                put("observability.metrics.path", Some(me.path.clone()));
            }
        }

        if let Some(mc) = rt.mcp.as_ref() {
            put("mcp.enabled", Some(mc.enabled.to_string()));
            put("mcp.allowed_hosts", Some(mc.allowed_hosts.join(" ")));
        }

        if let Some(o) = rt.oidc.as_ref() {
            put("oidc.issuer", o.issuer.clone());
            put("oidc.client_id", o.client_id.clone());
            put("oidc.redirect_url", o.redirect_url.clone());
            put("oidc.default_role", o.default_role.clone());
            put("oidc.provider_name", o.provider_name.clone());
            put("oidc.post_login_redirect", o.post_login_redirect.clone());
        }

        if let Some(s) = rt.smtp.as_ref() {
            put("smtp.host", s.host.clone());
            put("smtp.port", s.port.map(|p| p.to_string()));
            put("smtp.security", s.security.clone());
            put("smtp.from", s.from.clone());
        }

        put(
            "auth.password.enabled",
            rt.auth.password.enabled.map(|b| b.to_string()),
        );
        put(
            "auth.totp.required",
            rt.auth.totp.required.map(|b| b.to_string()),
        );

        // Alerts are cloned into the completion processor at boot, so channel
        // and rule edits are boot-only too. Rendered as a fingerprint, never as
        // values: a channel can carry an HMAC signing key, which must not reach
        // the log. The hash covers those keys (one-way) so rotating one is still
        // detected, while only the counts are ever printed.
        put("alerts", Some(alerts_fingerprint(&rt.alerts)));

        Self(m)
    }

    /// Settings that differ between the running snapshot (`self`) and a freshly
    /// parsed one, in stable path order.
    pub fn changes_vs(&self, pending: &Self) -> Vec<PendingRestart> {
        let mut keys: Vec<&'static str> = self.0.keys().copied().collect();
        keys.extend(pending.0.keys().copied());
        keys.sort_unstable();
        keys.dedup();

        keys.into_iter()
            .filter(|k| self.0.get(k) != pending.0.get(k))
            .map(|setting| PendingRestart {
                setting,
                running: self.0.get(setting).cloned(),
                pending: pending.0.get(setting).cloned(),
            })
            .collect()
    }
}

/// `N channel(s), M rule(s), fingerprint XXXXXXXX` — see the alerts note in
/// [`BootOnlySettings::from_runtime`].
fn alerts_fingerprint(alerts: &croniq_config::compile::AlertsConfig) -> String {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // `channels` is a HashMap, so hash it in name order for a stable result.
    let mut names: Vec<&String> = alerts.channels.keys().collect();
    names.sort();
    for name in &names {
        name.hash(&mut hasher);
        if let Some(ch) = alerts.channels.get(*name) {
            // Serde skips `signing_key` (it must never leak through
            // `/v1/dsl`), so feed the secret to the hasher separately —
            // otherwise a key rotation would look like "no change".
            serde_json::to_string(&ch.kind)
                .unwrap_or_default()
                .hash(&mut hasher);
            if let croniq_config::compile::ChannelKind::Webhook { signing_key, .. } = &ch.kind {
                signing_key.hash(&mut hasher);
            }
        }
    }
    for rule in &alerts.rules {
        serde_json::to_string(rule)
            .unwrap_or_default()
            .hash(&mut hasher);
    }
    format!(
        "{} channel(s), {} rule(s), fingerprint {:016x}",
        alerts.channels.len(),
        alerts.rules.len(),
        hasher.finish()
    )
}

/// WARN once per changed boot-only setting, naming the old and new value.
///
/// Called by every reload path (watcher, SIGHUP, admin endpoint) so a reload
/// that only *looks* successful can't hide a setting that didn't land.
pub fn log_pending_restart(changes: &[PendingRestart]) {
    for c in changes {
        tracing::warn!(
            setting = c.setting,
            running = c.running.as_deref().unwrap_or("<unset>"),
            pending = c.pending.as_deref().unwrap_or("<unset>"),
            "{} changed in the Croniqfile ({} → {}); this setting is applied at boot only and \
             needs a server restart to take effect",
            c.setting,
            c.running.as_deref().unwrap_or("<unset>"),
            c.pending.as_deref().unwrap_or("<unset>"),
        );
    }
}

/// A validated reload ready to apply.
pub struct ReloadPlan {
    /// DSL-only jobs (for `dsl_jobs_shared` update on apply).
    pub dsl_jobs: Vec<JobConfig>,
    /// DSL-only calendars (for `dsl_calendars_shared` update on apply).
    pub dsl_calendars: Vec<CalendarConfig>,
    /// Whether `policy { dsl_adopt_on_mutate true }` is set in the new file.
    /// Apply propagates this into `ServerState.policy_dsl_adopt_on_mutate`.
    pub policy_dsl_adopt_on_mutate: bool,
    /// Whether `policy { strict_calendars }` is set (default true) in the new
    /// file. Apply propagates this into `ServerState.policy_strict_calendars`
    /// so API handlers fail calendar references closed with the same policy
    /// the loader used (issue #393).
    pub policy_strict_calendars: bool,
    /// Merged jobs: DSL + API-registered (DSL wins on conflict).
    pub merged_jobs: Vec<JobConfig>,
    /// Merged triggers: DSL + API-registered (DSL wins on conflict).
    pub merged_triggers: HashMap<String, Trigger>,
    /// Jobs paused because their `calendar` reference did not resolve in the
    /// new file (issue #361). Apply replaces `ServerState.config_faults` with
    /// this so the fault set tracks the live config. Empty under
    /// `policy { strict_calendars false }`.
    pub calendar_faults: HashMap<String, String>,
    /// Summary of the difference vs. the running state.
    pub diff: ReloadDiff,
    /// The boot-only settings *as written in the reloaded file* (issue #406).
    /// Applying the plan does not apply these; callers compare them against the
    /// snapshot taken at boot ([`BootOnlySettings::changes_vs`]) to warn about
    /// the ones that need a restart. Kept out of [`ReloadDiff`] because the diff
    /// is what a reload changes, and these are exactly what it cannot.
    pub boot_only: BootOnlySettings,
}

/// Reload failure modes — shaped so the admin handler can surface them as
/// structured JSON.
#[derive(Debug)]
pub enum ReloadError {
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
    Validation {
        message: String,
        line: Option<usize>,
        column: Option<usize>,
    },
    Store(String),
}

impl std::fmt::Display for ReloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadFile { path, source } => {
                write!(f, "failed to read {}: {source}", path.display())
            }
            Self::Validation {
                message,
                line: Some(l),
                column: Some(c),
            } => write!(f, "validation failed at line {l}, column {c}: {message}"),
            Self::Validation { message, .. } => write!(f, "validation failed: {message}"),
            Self::Store(e) => write!(f, "store error: {e}"),
        }
    }
}

impl std::error::Error for ReloadError {}

/// Build a reload plan from a Croniqfile path.
///
/// Validates everything fully and returns an applicable plan, but does not
/// mutate scheduler or store state. Failure at any step aborts — existing
/// state is unchanged.
pub async fn build_plan(
    path: &Path,
    store: &DynStore,
    current_triggers: &RwLock<HashMap<String, Trigger>>,
    current_dsl_jobs: &RwLock<Vec<JobConfig>>,
) -> Result<ReloadPlan, ReloadError> {
    let loaded: LoadedConfig = load_file(path).map_err(|e| match e {
        LoadError::Io(err) => ReloadError::ReadFile {
            path: path.to_path_buf(),
            source: err,
        },
        LoadError::Parse {
            message,
            line,
            column,
        } => ReloadError::Validation {
            message,
            line,
            column,
        },
        LoadError::Schedule { job, reason } => ReloadError::Validation {
            message: format!("schedule error in job '{job}': {reason}"),
            line: None,
            column: None,
        },
        // An unknown IANA zone rejects the reload rather than quietly
        // re-arming the job in UTC (issue #426).
        LoadError::Timezone { job, reason } => ReloadError::Validation {
            message: format!("invalid timezone in job '{job}': {reason}"),
            line: None,
            column: None,
        },
        // Semantic errors (#402) reject the reload the same way a parse error
        // does — the running config stays untouched.
        LoadError::Validate { messages } => ReloadError::Validation {
            message: messages.join("; "),
            line: None,
            column: None,
        },
    })?;

    let now = Utc::now();
    let boot_only = BootOnlySettings::from_runtime(&loaded.runtime);

    // Adopted DSL keys are skipped on this reload — the API store wins.
    // Calendars and jobs are tracked under separate resource_type values
    // (`calendar`, `job`); here we only need the job-level set.
    let adopted_jobs: HashSet<String> = store
        .list_adoptions("job")
        .map_err(|e| ReloadError::Store(format!("{e}")))?
        .into_iter()
        .map(|a| a.resource_key)
        .collect();

    // Filter the DSL output: drop adopted entries before they enter the
    // merged plan, otherwise they'd shadow the API definitions.
    let dsl_jobs: Vec<JobConfig> = loaded
        .runtime
        .jobs
        .into_iter()
        .filter(|j| !adopted_jobs.contains(&j.key))
        .collect();
    let dsl_keys: HashSet<String> = dsl_jobs.iter().map(|j| j.key.clone()).collect();

    let mut merged_triggers: HashMap<String, Trigger> = loaded
        .triggers
        .into_iter()
        .filter(|(k, _)| !adopted_jobs.contains(k))
        .collect();
    let mut merged_jobs = dsl_jobs.clone();

    // Adopted DSL calendars are dropped from the DSL set — the API store
    // owns them now, so the resolver below picks up the store copy.
    let adopted_calendars: HashSet<String> = store
        .list_adoptions("calendar")
        .map_err(|e| ReloadError::Store(format!("{e}")))?
        .into_iter()
        .map(|a| a.resource_key)
        .collect();
    let dsl_calendars: Vec<CalendarConfig> = loaded
        .runtime
        .calendars
        .iter()
        .filter(|c| !adopted_calendars.contains(&c.name))
        .cloned()
        .collect();

    // Resolve calendar gates for API-registered triggers against the union
    // of DSL and store calendars (issue #393). Unresolvable references fail
    // closed under `strict_calendars`, mirroring the DSL faults collected by
    // the loader above.
    let resolved = crate::loader::resolve_calendars(
        &dsl_calendars,
        &store
            .list_calendars()
            .map_err(|e| ReloadError::Store(format!("{e}")))?,
        loaded.runtime.policy.strict_calendars,
    );

    let mut api_calendar_faults: HashMap<String, String> = HashMap::new();
    let api_triggers = store
        .list_triggers(None)
        .map_err(|e| ReloadError::Store(format!("{e}")))?;
    for def in &api_triggers {
        if def.managed_by == "dsl" || !def.enabled {
            continue;
        }
        if dsl_keys.contains(&def.job_key) {
            // DSL precedence — drop API trigger for this key.
            continue;
        }
        if let Some(built) = trigger_from_definition(def, &resolved, now) {
            let job_config = job_config_from_definition(def, None);
            merged_jobs.push(job_config);
            merged_triggers.insert(def.job_key.clone(), built.trigger);
            if let Some(reason) = built.config_fault {
                api_calendar_faults.insert(def.job_key.clone(), reason);
            }
        }
    }

    // Diff against the current scheduler state.
    let diff = {
        let current = current_triggers.read().await;
        let current_dsl = current_dsl_jobs.read().await;

        let current_keys: HashSet<String> = current.keys().cloned().collect();
        let merged_keys: HashSet<String> = merged_triggers.keys().cloned().collect();

        let mut added: Vec<String> = merged_keys.difference(&current_keys).cloned().collect();
        added.sort();

        let mut removed: Vec<String> = current_keys.difference(&merged_keys).cloned().collect();
        removed.sort();

        let current_dsl_by_key: HashMap<&str, &JobConfig> =
            current_dsl.iter().map(|j| (j.key.as_str(), j)).collect();
        let mut changed: Vec<String> = Vec::new();
        for new_job in &merged_jobs {
            if let Some(old_job) = current_dsl_by_key.get(new_job.key.as_str())
                && job_changed(old_job, new_job)
            {
                changed.push(new_job.key.clone());
            }
        }
        changed.sort();

        ReloadDiff {
            added,
            removed,
            changed,
            total: merged_triggers.len(),
        }
    };

    // Keep only faults for jobs that survived into the merged plan (an adopted
    // job's DSL trigger is dropped, so its fault is moot), then add the faults
    // collected for API-registered triggers above. Apply replaces the fault
    // set wholesale, so every reload must recompute both sources.
    let mut calendar_faults: HashMap<String, String> = loaded
        .calendar_faults
        .into_iter()
        .filter(|(k, _)| merged_triggers.contains_key(k))
        .collect();
    calendar_faults.extend(api_calendar_faults);

    Ok(ReloadPlan {
        dsl_jobs,
        dsl_calendars,
        policy_dsl_adopt_on_mutate: loaded.runtime.policy.dsl_adopt_on_mutate,
        policy_strict_calendars: loaded.runtime.policy.strict_calendars,
        merged_jobs,
        merged_triggers,
        calendar_faults,
        diff,
        boot_only,
    })
}

/// Structural check for scheduling-relevant changes. Intentionally brittle —
/// covers the fields that actually alter runtime behaviour.
fn job_changed(a: &JobConfig, b: &JobConfig) -> bool {
    a.schedule_summary != b.schedule_summary
        || a.timezone != b.timezone
        || a.timeout != b.timeout
        || a.calendar != b.calendar
        || a.window != b.window
        || a.not_before != b.not_before
        || a.not_after != b.not_after
        || a.retry.max_attempts != b.retry.max_attempts
        || a.execution_mode != b.execution_mode
        || a.catch_up != b.catch_up
        || a.dead_letter.enabled != b.dead_letter.enabled
}

/// Apply a validated reload plan from outside the scheduler task.
///
/// Sends a `SchedulerCommand::Reload` and waits for the scheduler to ack.
/// After the ack, updates the shared DSL job/calendar lists and trigger
/// snapshot so subsequent API reads and reload diffs see the new state.
#[allow(clippy::too_many_arguments)]
pub async fn apply_plan(
    plan: ReloadPlan,
    scheduler_tx: &mpsc::UnboundedSender<SchedulerCommand>,
    dsl_jobs_shared: &RwLock<Vec<JobConfig>>,
    dsl_calendars_shared: &RwLock<Vec<CalendarConfig>>,
    policy_dsl_adopt: &std::sync::atomic::AtomicBool,
    policy_strict_calendars: &std::sync::atomic::AtomicBool,
    trigger_snapshot: &RwLock<HashMap<String, Trigger>>,
    config_faults: &std::sync::RwLock<HashMap<String, String>>,
) -> Result<(), ApplyError> {
    let post_triggers = plan.merged_triggers.clone();
    let post_dsl = plan.dsl_jobs;
    let post_dsl_cals = plan.dsl_calendars;
    let post_policy = plan.policy_dsl_adopt_on_mutate;
    let post_strict = plan.policy_strict_calendars;
    let post_faults = plan.calendar_faults;

    let (ack_tx, ack_rx) = oneshot::channel();
    scheduler_tx
        .send(SchedulerCommand::Reload {
            triggers: plan.merged_triggers,
            jobs: plan.merged_jobs,
            ack: ack_tx,
        })
        .map_err(|_| ApplyError::SchedulerDown)?;

    ack_rx.await.map_err(|_| ApplyError::SchedulerDown)?;

    *dsl_jobs_shared.write().await = post_dsl;
    *dsl_calendars_shared.write().await = post_dsl_cals;
    policy_dsl_adopt.store(post_policy, std::sync::atomic::Ordering::Relaxed);
    policy_strict_calendars.store(post_strict, std::sync::atomic::Ordering::Relaxed);
    *trigger_snapshot.write().await = post_triggers;
    *config_faults.write().unwrap() = post_faults;
    Ok(())
}

/// Apply a reload plan directly from inside the scheduler task.
///
/// Use this from the scheduler select-loop when you already own `&mut
/// SchedulerLoop` — sending a command + awaiting the ack from within the
/// scheduler task itself would deadlock.
#[allow(clippy::too_many_arguments)]
pub async fn apply_plan_direct(
    plan: ReloadPlan,
    scheduler: &mut SchedulerLoop,
    dsl_jobs_shared: &RwLock<Vec<JobConfig>>,
    dsl_calendars_shared: &RwLock<Vec<CalendarConfig>>,
    policy_dsl_adopt: &std::sync::atomic::AtomicBool,
    policy_strict_calendars: &std::sync::atomic::AtomicBool,
    trigger_snapshot: &RwLock<HashMap<String, Trigger>>,
    config_faults: &std::sync::RwLock<HashMap<String, String>>,
) {
    let post_triggers = plan.merged_triggers.clone();
    let post_policy = plan.policy_dsl_adopt_on_mutate;
    let post_strict = plan.policy_strict_calendars;
    let post_faults = plan.calendar_faults;
    scheduler.reload(plan.merged_triggers, plan.merged_jobs);
    *dsl_jobs_shared.write().await = plan.dsl_jobs;
    *dsl_calendars_shared.write().await = plan.dsl_calendars;
    policy_dsl_adopt.store(post_policy, std::sync::atomic::Ordering::Relaxed);
    policy_strict_calendars.store(post_strict, std::sync::atomic::Ordering::Relaxed);
    *trigger_snapshot.write().await = post_triggers;
    *config_faults.write().unwrap() = post_faults;
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("scheduler task is not running")]
    SchedulerDown,
}

/// Atomic counters for `croniq_config_reload_total`.
#[derive(Debug, Default)]
pub struct ReloadCounters {
    pub success: AtomicU64,
    pub validation_error: AtomicU64,
    pub apply_error: AtomicU64,
}

impl ReloadCounters {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn inc_success(&self) {
        self.success.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_validation_error(&self) {
        self.validation_error.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_apply_error(&self) {
        self.apply_error.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::load_str;
    use crate::store::sqlite_store;
    use croniq_store::sqlite::SqliteStore;

    fn empty_store() -> DynStore {
        sqlite_store(SqliteStore::in_memory().unwrap())
    }

    async fn state_from(src: &str) -> (RwLock<HashMap<String, Trigger>>, RwLock<Vec<JobConfig>>) {
        let loaded = load_str(src).unwrap();
        (
            RwLock::new(loaded.triggers),
            RwLock::new(loaded.runtime.jobs),
        )
    }

    // ── Boot-only settings (issue #406) ─────────────────────────────────

    fn boot_only_of(src: &str) -> BootOnlySettings {
        BootOnlySettings::from_runtime(&load_str(src).unwrap().runtime)
    }

    /// The one setting from the issue's own example, end to end.
    #[tokio::test]
    async fn changed_retention_is_reported_as_pending_restart() {
        let running =
            boot_only_of("server { execution_retention 30d }\njob a:one { every 1 hours }");

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            tmp.path(),
            "server { execution_retention 90d }\njob a:one { every 1 hours }",
        )
        .unwrap();
        let (cur_tr, cur_dsl) = state_from("job a:one { every 1 hours }").await;
        let store = empty_store();
        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        assert_eq!(
            running.changes_vs(&plan.boot_only),
            vec![PendingRestart {
                setting: "server.execution_retention",
                running: Some("30d".into()),
                pending: Some("90d".into()),
            }]
        );
        // The jobs are untouched, so the reload itself is a no-op — which is
        // exactly the case where a plain "reloaded" log used to be misleading.
        assert!(plan.diff.is_noop());
    }

    #[test]
    fn job_only_edits_report_nothing_pending() {
        let running = boot_only_of("server { listen :4000 }\njob a:one { every 1 hours }");
        let pending = boot_only_of("server { listen :4000 }\njob a:one { every 30 minutes }");
        assert!(running.changes_vs(&pending).is_empty());
    }

    #[test]
    fn identical_config_reports_nothing_pending() {
        let src = r#"
            server { listen :4000; app_url "https://x.test"; execution_retention 30d }
            pull_api { lease_ttl 60s }
            observability { log { level info } metrics { listen :9900; path /metrics } }
            mcp { enabled true; allowed_hosts a.test }
            smtp { host smtp.test; port 587 }
            auth { totp { required true } }
            job a:one { every 1 hours }
        "#;
        assert!(boot_only_of(src).changes_vs(&boot_only_of(src)).is_empty());
    }

    #[test]
    fn added_and_removed_settings_are_distinguishable() {
        let bare = boot_only_of("job a:one { every 1 hours }");
        let with_url =
            boot_only_of("server { app_url \"https://x.test\" }\njob a:one { every 1 hours }");

        assert_eq!(
            bare.changes_vs(&with_url),
            vec![PendingRestart {
                setting: "server.app_url",
                running: None,
                pending: Some("https://x.test".into()),
            }]
        );
        // …and the other direction: dropping the line is a change too.
        assert_eq!(
            with_url.changes_vs(&bare),
            vec![PendingRestart {
                setting: "server.app_url",
                running: Some("https://x.test".into()),
                pending: None,
            }]
        );
    }

    #[test]
    fn every_boot_only_section_is_covered() {
        // One edit per section, to catch a section that was never flattened.
        let base = r#"
            server { listen :4000; data_dir /a; db sqlite }
            pull_api { lease_ttl 60s }
            observability { log { level info } metrics { listen :9900; path /m } }
            mcp { enabled true }
            oidc { issuer "https://a.test" }
            smtp { host a.test }
            auth { password { enabled true } totp { required false } }
            job a:one { every 1 hours }
        "#;
        let running = boot_only_of(base);
        for (label, edited) in [
            ("server.listen", base.replace(":4000", ":4001")),
            ("server.data_dir", base.replace("/a", "/b")),
            ("server.db", base.replace("db sqlite", "db postgres://x/y")),
            ("pull_api.lease_ttl", base.replace("60s", "90s")),
            (
                "observability.log.level",
                base.replace("level info", "level debug"),
            ),
            (
                "observability.metrics.path",
                base.replace("/m", "/metrics2"),
            ),
            ("mcp.enabled", base.replace("enabled true", "enabled false")),
            (
                "oidc.issuer",
                base.replace("https://a.test", "https://b.test"),
            ),
            ("smtp.host", base.replace("host a.test", "host b.test")),
            (
                "auth.totp.required",
                base.replace("required false", "required true"),
            ),
        ] {
            let changes = running.changes_vs(&boot_only_of(&edited));
            assert!(
                changes.iter().any(|c| c.setting == label),
                "editing {label} produced {changes:?}"
            );
        }
    }

    #[test]
    fn alerts_changes_are_detected_without_logging_values() {
        let base = r#"
            alerts {
              channel "ops" { webhook "https://hooks.test/a"
                              sign hmac super-secret }
              rule "fail" { when job_failed; channels "ops" }
            }
            job a:one { every 1 hours }
        "#;
        let running = boot_only_of(base);

        // A rotated signing key must still register as a change even though
        // serde skips the field…
        let rotated = base.replace("super-secret", "rotated-secret");
        let changes = running.changes_vs(&boot_only_of(&rotated));
        assert_eq!(changes.len(), 1, "got: {changes:?}");
        assert_eq!(changes[0].setting, "alerts");
        // …and neither the old nor the new secret may appear in what we print.
        let rendered = format!("{changes:?}");
        assert!(!rendered.contains("super-secret"), "leaked: {rendered}");
        assert!(!rendered.contains("rotated-secret"), "leaked: {rendered}");
        assert!(
            rendered.contains("1 channel(s), 1 rule(s)"),
            "got: {rendered}"
        );

        // An unrelated edit to the URL is a change as well.
        let moved = base.replace("hooks.test/a", "hooks.test/b");
        assert_eq!(running.changes_vs(&boot_only_of(&moved)).len(), 1);
    }

    #[tokio::test]
    async fn diff_detects_added_and_removed() {
        let (cur_tr, cur_dsl) = state_from("job a:one { every 1 hours }").await;

        // Write new config to a temp file.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "job b:two { every 1 hours }").unwrap();

        let store = empty_store();
        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        assert_eq!(plan.diff.added, vec!["b:two"]);
        assert_eq!(plan.diff.removed, vec!["a:one"]);
        assert!(plan.diff.changed.is_empty());
        assert_eq!(plan.diff.total, 1);
    }

    #[tokio::test]
    async fn diff_detects_schedule_change() {
        let (cur_tr, cur_dsl) = state_from("job a:one { every 1 hours }").await;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "job a:one { every 30 minutes }").unwrap();

        let store = empty_store();
        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        assert!(plan.diff.added.is_empty());
        assert!(plan.diff.removed.is_empty());
        assert_eq!(plan.diff.changed, vec!["a:one"]);
    }

    #[tokio::test]
    async fn diff_noop_when_identical() {
        let src = "job a:one { every 1 hours }";
        let (cur_tr, cur_dsl) = state_from(src).await;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), src).unwrap();

        let store = empty_store();
        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        assert!(plan.diff.is_noop());
    }

    #[tokio::test]
    async fn invalid_file_returns_validation_error_with_line_col() {
        let (cur_tr, cur_dsl) = state_from("job a:one { every 1 hours }").await;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        // Intentional garbage on line 2.
        std::fs::write(tmp.path(), "\n@@@not valid DSL@@@\n").unwrap();

        let store = empty_store();
        let err = match build_plan(tmp.path(), &store, &cur_tr, &cur_dsl).await {
            Ok(_) => panic!("expected validation error"),
            Err(e) => e,
        };

        match err {
            ReloadError::Validation { line, column, .. } => {
                assert!(line.is_some(), "line should be populated from span");
                assert!(column.is_some(), "column should be populated from span");
            }
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn missing_file_returns_read_error() {
        let (cur_tr, cur_dsl) = state_from("job a:one { every 1 hours }").await;
        let store = empty_store();
        let err = match build_plan(
            Path::new("this-file-does-not-exist-42.croniq"),
            &store,
            &cur_tr,
            &cur_dsl,
        )
        .await
        {
            Ok(_) => panic!("expected read error"),
            Err(e) => e,
        };
        assert!(matches!(err, ReloadError::ReadFile { .. }));
    }

    #[tokio::test]
    async fn api_triggers_merged_when_not_in_dsl() {
        let (cur_tr, cur_dsl) = state_from("job dsl:only { every 1 hours }").await;

        // Seed an API-registered trigger in the store.
        let store = empty_store();
        store
            .create_trigger(&croniq_store::models::TriggerDefinition {
                trigger_id: "api-1".into(),
                job_key: "api:kept".into(),
                cron_expression: Some("5m".into()),
                timezone: None,
                calendar: None,
                window: None,
                not_before: None,
                not_after: None,
                enabled: true,
                managed_by: "api".into(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .unwrap();

        let tmp = tempfile::NamedTempFile::new().unwrap();
        // New DSL drops the old DSL job.
        std::fs::write(tmp.path(), "job dsl:new { every 2 hours }").unwrap();

        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        let keys: HashSet<&str> = plan.merged_triggers.keys().map(|s| s.as_str()).collect();
        assert!(keys.contains("dsl:new"), "DSL job survives");
        assert!(
            keys.contains("api:kept"),
            "API-registered job survives reload"
        );
        assert!(!keys.contains("dsl:only"), "old DSL job is removed");
    }

    #[tokio::test]
    async fn reload_rebuilds_adopted_non_interval_triggers() {
        // After adoption a job lives only in the store (managed_by="api"), so a
        // reload must rebuild its runtime trigger from the persisted canonical
        // schedule expression. Before the fix `trigger_from_definition` only
        // understood interval shorthand, so every adopted daily/weekly/once job
        // silently vanished from the scheduler on the next reload/restart
        // (found while fixing #393).
        let (cur_tr, cur_dsl) = state_from("").await;
        let store = empty_store();

        let adopted = [
            ("adopted:daily", "every day at 02:00"),
            ("adopted:weekly", "every monday friday at 09:00"),
            ("adopted:once", r#"once at "2999-01-01T00:00:00Z""#),
        ];
        for (key, cron) in adopted {
            store
                .create_trigger(&croniq_store::models::TriggerDefinition {
                    trigger_id: format!("api-{key}"),
                    job_key: key.into(),
                    cron_expression: Some(cron.into()),
                    timezone: None,
                    calendar: None,
                    window: None,
                    not_before: None,
                    not_after: None,
                    enabled: true,
                    managed_by: "api".into(),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                })
                .unwrap();
        }

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "").unwrap();

        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        for (key, _) in adopted {
            let trigger = plan
                .merged_triggers
                .get(key)
                .unwrap_or_else(|| panic!("{key} was not rebuilt on reload"));
            assert!(
                trigger.next_fire_at.is_some(),
                "{key}: rebuilt trigger has no next fire time"
            );
        }
    }

    #[tokio::test]
    async fn dsl_wins_on_conflict_with_api_trigger() {
        let (cur_tr, cur_dsl) = state_from("job shared:key { every 1 hours }").await;

        let store = empty_store();
        store
            .create_trigger(&croniq_store::models::TriggerDefinition {
                trigger_id: "api-1".into(),
                job_key: "shared:key".into(),
                cron_expression: Some("5m".into()),
                timezone: None,
                calendar: None,
                window: None,
                not_before: None,
                not_after: None,
                enabled: true,
                managed_by: "api".into(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .unwrap();

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "job shared:key { every 30 minutes }").unwrap();

        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        // Only one entry for the key, and its summary is the DSL's.
        let job = plan
            .merged_jobs
            .iter()
            .find(|j| j.key == "shared:key")
            .unwrap();
        assert_eq!(job.schedule_summary, "every 30 minutes");
        assert_eq!(
            plan.merged_jobs
                .iter()
                .filter(|j| j.key == "shared:key")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn adopted_dsl_calendar_is_skipped_in_plan() {
        let (cur_tr, cur_dsl) = state_from("").await;
        let store = empty_store();

        // Mark `business-days` as adopted — the loader should drop it from
        // the DSL output even though the Croniqfile still defines it.
        store
            .insert_adoption(&croniq_store::models::DslAdoption {
                resource_type: "calendar".into(),
                resource_key: "business-days".into(),
                adopted_at: Utc::now(),
                adopted_by: None,
            })
            .unwrap();

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            tmp.path(),
            r#"
            calendar business-days {
              include weekly monday
            }
            calendar still-dsl {
              include weekly tuesday
            }
            "#,
        )
        .unwrap();

        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        let names: Vec<&str> = plan.dsl_calendars.iter().map(|c| c.name.as_str()).collect();
        assert!(
            !names.contains(&"business-days"),
            "adopted DSL calendar must be skipped"
        );
        assert!(
            names.contains(&"still-dsl"),
            "non-adopted DSL calendar still surfaces"
        );
    }

    #[tokio::test]
    async fn adopted_dsl_job_is_skipped_in_plan() {
        let (cur_tr, cur_dsl) = state_from("").await;
        let store = empty_store();

        store
            .insert_adoption(&croniq_store::models::DslAdoption {
                resource_type: "job".into(),
                resource_key: "billing:invoice".into(),
                adopted_at: Utc::now(),
                adopted_by: None,
            })
            .unwrap();

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            tmp.path(),
            "job billing:invoice { every 1 hours }\njob etl:sync { every 5 minutes }\n",
        )
        .unwrap();

        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        let keys: Vec<&str> = plan.dsl_jobs.iter().map(|j| j.key.as_str()).collect();
        assert!(
            !keys.contains(&"billing:invoice"),
            "adopted job dropped from DSL set"
        );
        assert!(
            keys.contains(&"etl:sync"),
            "non-adopted job still in DSL set"
        );
        assert!(
            !plan.merged_triggers.contains_key("billing:invoice"),
            "adopted job's trigger must not appear in merged set"
        );
    }

    #[tokio::test]
    async fn policy_dsl_adopt_on_mutate_propagates() {
        let (cur_tr, cur_dsl) = state_from("").await;
        let store = empty_store();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            tmp.path(),
            "policy { dsl_adopt_on_mutate true }\njob a:b { every 1 hours }\n",
        )
        .unwrap();
        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();
        assert!(plan.policy_dsl_adopt_on_mutate);
    }

    #[tokio::test]
    async fn reload_with_broken_calendar_pauses_job_not_ungated() {
        // A previously-armed job whose calendar becomes broken on reload must
        // fail closed (paused), not keep firing without its gate (issue #361).
        let (cur_tr, cur_dsl) = state_from(
            "calendar biz { include weekly monday }\njob a:b { every 1 minutes { calendar biz } }",
        )
        .await;
        assert_ne!(
            cur_tr.read().await["a:b"].state,
            croniq_scheduler::trigger::TriggerState::Paused
        );

        let store = empty_store();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            tmp.path(),
            "calendar biz { include weekly funday }\njob a:b { every 1 minutes { calendar biz } }\n",
        )
        .unwrap();
        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        assert_eq!(
            plan.merged_triggers["a:b"].state,
            croniq_scheduler::trigger::TriggerState::Paused
        );
        assert!(plan.calendar_faults.contains_key("a:b"));
    }

    /// Helper: seed an API trigger referencing a calendar name.
    fn seed_api_trigger_cal(store: &DynStore, job_key: &str, calendar: Option<&str>) {
        store
            .create_trigger(&croniq_store::models::TriggerDefinition {
                trigger_id: format!("tid-{job_key}"),
                job_key: job_key.into(),
                cron_expression: Some("1m".into()),
                timezone: None,
                calendar: calendar.map(|c| c.into()),
                window: None,
                not_before: None,
                not_after: None,
                enabled: true,
                managed_by: "api".into(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .unwrap();
    }

    /// Helper: seed an API (store) calendar.
    fn seed_store_calendar(store: &DynStore, name: &str, rules: &str) {
        store
            .create_calendar(&croniq_store::models::CalendarDefinition {
                calendar_id: format!("cid-{name}"),
                name: name.into(),
                timezone: None,
                rules: rules.into(),
                managed_by: "api".into(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .unwrap();
    }

    #[tokio::test]
    async fn reload_attaches_store_calendar_to_api_trigger() {
        // #393: an API trigger referencing a store calendar must come back
        // gated after a reload, not un-gated.
        let (cur_tr, cur_dsl) = state_from("job dsl:x { every 1 hours }").await;
        let store = empty_store();
        seed_store_calendar(&store, "weekdays", "include weekly weekday");
        seed_api_trigger_cal(&store, "api:gated", Some("weekdays"));

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "job dsl:x { every 1 hours }").unwrap();
        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        assert!(
            plan.merged_triggers["api:gated"].calendar.is_some(),
            "store calendar must be attached on reload"
        );
        assert!(!plan.calendar_faults.contains_key("api:gated"));
    }

    #[tokio::test]
    async fn reload_faults_api_trigger_with_unknown_calendar_under_strict() {
        let (cur_tr, cur_dsl) = state_from("job dsl:x { every 1 hours }").await;
        let store = empty_store();
        seed_api_trigger_cal(&store, "api:gated", Some("ghost"));

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "job dsl:x { every 1 hours }").unwrap();
        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        assert_eq!(
            plan.merged_triggers["api:gated"].state,
            croniq_scheduler::trigger::TriggerState::Paused
        );
        assert!(plan.calendar_faults.contains_key("api:gated"));
        assert!(plan.policy_strict_calendars);
    }

    #[tokio::test]
    async fn reload_api_trigger_unknown_calendar_lenient_runs_ungated() {
        let (cur_tr, cur_dsl) = state_from("job dsl:x { every 1 hours }").await;
        let store = empty_store();
        seed_api_trigger_cal(&store, "api:gated", Some("ghost"));

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            tmp.path(),
            "policy { strict_calendars false }\njob dsl:x { every 1 hours }\n",
        )
        .unwrap();
        let plan = build_plan(tmp.path(), &store, &cur_tr, &cur_dsl)
            .await
            .unwrap();

        assert_ne!(
            plan.merged_triggers["api:gated"].state,
            croniq_scheduler::trigger::TriggerState::Paused
        );
        assert!(plan.merged_triggers["api:gated"].calendar.is_none());
        assert!(!plan.calendar_faults.contains_key("api:gated"));
        assert!(!plan.policy_strict_calendars);
    }
}
