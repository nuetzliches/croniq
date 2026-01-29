import os
import sys
import asyncio
import signal
from pathlib import Path

sdk_root = Path(__file__).resolve().parents[4] / "sdk" / "runner-python"
sys.path.insert(0, str(sdk_root))

from croniq_runner import (
    CroniqRunner,
    RunnerConfig,
    RunnerIdInUseError,
    RunnerJobRegistrationDeniedError,
    RunnerLogger,
    RunnerJobRegistration,
)


async def main() -> None:
    runner_api_key = os.getenv("CRONIQ_RUNNER_PYTHON_API_KEY", "").strip()
    api_key = os.getenv("CRONIQ_API_KEY", "").strip()
    if not api_key and runner_api_key:
        os.environ["CRONIQ_API_KEY"] = runner_api_key
    runner_id = os.getenv("CRONIQ_RUNNER_ID", "").strip()
    if not runner_id or (runner_id.lower() == "default" and runner_api_key):
        os.environ["CRONIQ_RUNNER_ID"] = "python-default" if runner_api_key else "default"

    try:
        config = RunnerConfig.from_env()
    except ValueError as exc:
        print(f"invalid runner config: {exc}")
        raise

    runner = CroniqRunner(config)
    job_key = os.getenv("CRONIQ_JOB_KEY", "samples:python-job").strip()

    print("Croniq runner (python)")
    print(f"- base_url:    {config.base_url}")
    print(f"- grpc_url:    {config.grpc_base_url or config.base_url}")
    print(f"- tenant_id:   {config.tenant_id}")
    print(f"- environment: {config.environment}")
    print(f"- runner_id:   {config.runner_id}")
    if config.runner_instance_id:
        print(f"- runner_instance: {config.runner_instance_id}")
    if job_key:
        print(f"- job_key:     {job_key}")

    async def handle_execution(context, payload, logger: RunnerLogger) -> None:
        logger.info(
            "execution started",
            {
                "executionId": context.execution_id,
                "jobKey": context.job_key,
                "triggerId": context.trigger_id,
                "mode": context.execution_mode,
            },
        )

        if payload is not None:
            print("payload received", payload)

        logger.info("execution completed", {"executionId": context.execution_id})

    runner.on_execute(
        job_key,
        handle_execution,
        RunnerJobRegistration(
            description="Demo job registered by the Python runner sample.",
            metadata={"sample": "python", "sdk": "croniq-runner"},
        ),
    )

    stop_event = asyncio.Event()
    loop = asyncio.get_running_loop()

    def on_signal() -> None:
        stop_event.set()

    for sig in (signal.SIGTERM, signal.SIGINT):
        try:
            loop.add_signal_handler(sig, on_signal)
        except (NotImplementedError, RuntimeError):
            try:
                signal.signal(sig, lambda *_: on_signal())
            except (ValueError, OSError):
                continue

    async def drain_on_signal() -> None:
        await stop_event.wait()
        print("runner draining due to signal")
        await runner.drain(30000)

    try:
        await asyncio.gather(runner.start(), drain_on_signal())
    except RunnerIdInUseError as exc:
        print(f"runnerId already in use: {exc}")
        raise
    except RunnerJobRegistrationDeniedError as exc:
        print(f"job registration denied: {exc}")
        raise


if __name__ == "__main__":
    asyncio.run(main())
