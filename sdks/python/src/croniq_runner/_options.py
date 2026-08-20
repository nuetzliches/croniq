"""Runner and log-writer configuration."""

from __future__ import annotations

from dataclasses import dataclass, field

from croniq_runner._security import validate_server_url


@dataclass(slots=True)
class LogWriterOptions:
    """Tunables for the streaming :class:`LogWriter`.

    Defaults mirror the Rust and .NET runner SDKs so observable behaviour stays
    consistent across implementations.
    """

    channel_capacity: int = 256
    """Bounded queue capacity. Backpressure (await) kicks in when full."""

    batch_size_threshold: int = 32
    """Flush when this many events have accumulated."""

    batch_time_threshold_ms: int = 200
    """Flush at least this often, even if the size threshold isn't reached."""

    max_batch_per_post: int = 100
    """Maximum events per outgoing HTTP POST."""

    shutdown_timeout_ms: int = 5000
    """How long the runner waits for queued events to flush before sending the ack."""


@dataclass(slots=True)
class RunnerOptions:
    """Configuration for a single Croniq runner instance."""

    server_url: str = "http://localhost:4000"
    """Base URL of the Croniq server.

    ``https://`` is required unless the host is loopback (``localhost``,
    ``127.0.0.0/8``, ``::1``) — the API key is attached to every request and
    would otherwise travel in cleartext. See :attr:`allow_insecure_http`.
    """

    runner_id: str | None = None
    """Stable runner identifier. Falls back to ``$RUNNER_ID`` / a generated value."""

    runner_id_prefix: str = "runner"
    """Prefix used when generating a fresh runner ID."""

    api_key: str | None = None
    """API key for ``Authorization: ApiKey <key>``. Takes precedence over bearer."""

    bearer_token: str | None = None
    """Bearer token for ``Authorization: Bearer <token>``."""

    capabilities: list[str] = field(default_factory=list)
    """Capabilities the runner advertises (used by server-side routing)."""

    tags: list[str] = field(default_factory=list)
    """Free-form key=value tags self-declared by the runner. Filter-only."""

    max_inflight: int = 5
    """Maximum concurrent in-flight executions."""

    poll_timeout_ms: int = 35_000
    """Per-request timeout for the long-poll work endpoint."""

    renew_interval_ms: int = 15_000
    """Heartbeat interval for in-flight lease renewals."""

    drain_timeout_ms: int = 30_000
    """How long :meth:`Runner.run` waits for in-flight executions on shutdown."""

    poll_retry_delay_ms: int = 5_000
    """Back-off after a failed poll request."""

    capacity_backoff_ms: int = 500
    """Idle delay when the runner is at ``max_inflight`` capacity."""

    max_consecutive_poll_conflicts: int = 3
    """How many consecutive ``409 Conflict`` poll responses to tolerate.

    On exhaustion :meth:`Runner.run` raises
    :class:`~croniq_runner.PollInstanceConflictError` instead of retrying
    forever: a sustained ``409`` means a second process is registered under
    the same ``runner_id`` and no amount of retrying fixes that. The counter
    resets on a successful poll or on any non-409 failure (5xx, network,
    timeout), which say nothing about instance ownership.
    """

    max_consecutive_auth_failures: int = 3
    """How many consecutive ``401 Unauthorized`` poll responses to tolerate.

    On exhaustion :meth:`Runner.run` raises
    :class:`~croniq_runner.AuthFailedError` instead of retrying forever: the
    API key is read once and never re-read, so a rejected credential cannot
    fix itself, and a runner that keeps polling looks idle rather than broken
    (issue #473). Not fatal on the first ``401`` — rotation hands over through
    an expiry window (server issue #471) and a race around it should not kill
    a healthy runner. The counter resets on a successful poll and on any other
    failure: a 5xx says nothing about whether the credential is valid.
    """

    log_writer: LogWriterOptions = field(default_factory=LogWriterOptions)

    allow_insecure_http: bool = False
    """Opt in to a cleartext ``http://`` :attr:`server_url` on a non-loopback host.

    Off by default: without it such a URL is refused at construction time. When
    enabled the SDK still emits one loud startup warning, because the API key
    then travels in cleartext on every poll.
    """

    def __post_init__(self) -> None:
        validate_server_url(
            self.server_url,
            allow_insecure_http=self.allow_insecure_http,
            option_name="RunnerOptions",
        )
        # Guarded where the neighbouring tunables are not, because the
        # out-of-range value here is fatal rather than merely odd: 0 would
        # make the runner exit on its very first 409, which reads as a
        # crash-loop rather than a misconfiguration.
        if not 1 <= self.max_consecutive_poll_conflicts <= 100:
            raise ValueError(
                "RunnerOptions.max_consecutive_poll_conflicts must be in [1, 100], "
                f"got {self.max_consecutive_poll_conflicts}"
            )
        if not 1 <= self.max_consecutive_auth_failures <= 100:
            raise ValueError(
                "RunnerOptions.max_consecutive_auth_failures must be in [1, 100], "
                f"got {self.max_consecutive_auth_failures}"
            )
