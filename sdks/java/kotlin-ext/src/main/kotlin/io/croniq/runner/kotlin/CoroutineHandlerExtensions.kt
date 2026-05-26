package io.croniq.runner.kotlin

import io.croniq.runner.CroniqRunner
import io.croniq.runner.handler.CroniqCancellation
import io.croniq.runner.handler.CroniqExecutionContext
import io.croniq.runner.handler.CroniqJobHandler
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.job
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking

/**
 * Coroutine-friendly version of [CroniqRunner.Builder.addJob]. The supplied
 * handler is `suspend`; the SDK runs each execution on a virtual thread via
 * [runBlocking] so blocking the virtual thread is cheap and `Dispatchers.IO`
 * stays available for any nested I/O calls.
 *
 * Cancellation: a watcher coroutine polls the server-side [CroniqCancellation]
 * flag and cancels the coroutine job when the server requests a cancel —
 * standard `coroutineContext.isActive` / `ensureActive()` reads in the handler
 * see the cancel naturally.
 */
public fun CroniqRunner.Builder.addJob(
    jobKey: String,
    handler: suspend (CroniqExecutionContext) -> Unit,
): CroniqRunner.Builder = addJob(jobKey, null, handler)

/**
 * Variant with a schedule string — the runner self-registers the job at
 * startup, equivalent to the Java overload
 * [CroniqRunner.Builder.addJob] with the schedule parameter.
 */
public fun CroniqRunner.Builder.addJob(
    jobKey: String,
    schedule: String?,
    handler: suspend (CroniqExecutionContext) -> Unit,
): CroniqRunner.Builder {
    val javaHandler =
        CroniqJobHandler { ctx ->
            runBlocking(Dispatchers.IO) {
                val outerJob: Job = coroutineContext.job
                val watcher = launchCancelWatcher(ctx.cancellation(), outerJob)
                try {
                    handler(ctx)
                } finally {
                    watcher.cancel()
                }
            }
        }
    return if (schedule.isNullOrBlank()) {
        addJob(jobKey, javaHandler)
    } else {
        addJob(jobKey, schedule, javaHandler)
    }
}

private fun CoroutineScope.launchCancelWatcher(
    cancellation: CroniqCancellation,
    target: Job,
) = launch {
    while (isActive && !cancellation.isRequested) {
        delay(POLL_INTERVAL_MS)
    }
    if (cancellation.isRequested) {
        // Cancel the runBlocking root job — the handler's suspending
        // calls (delay / IO / withContext / …) unwind via the resulting
        // CancellationException.
        target.cancel("Execution cancelled by Croniq server")
    }
}

// 50 ms is small enough that handlers in tight loops see the cancel within
// a single poll cycle, and infrequent enough that a long-running handler
// doesn't accumulate measurable wake-up overhead.
private const val POLL_INTERVAL_MS = 50L
