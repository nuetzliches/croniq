package io.croniq.runner;

/**
 * Entry point for the Croniq runner. Polls work, dispatches handlers, streams
 * logs, and reports completion.
 *
 * <p>This is a stub. The full implementation lands in PR-2 (poll/ack loop) and
 * later phases of issue #133. The class is here at PR-1 so module dependencies
 * and the public surface name are pinned by the skeleton.
 */
public final class CroniqRunner {

    private CroniqRunner() {
        // Builders / factory methods land in PR-2.
    }

    /**
     * Returns the SDK version baked in at build time. Used by the conformance
     * binding's {@code runner_id} composition and by the {@code User-Agent}
     * header on outbound HTTP requests.
     */
    public static String sdkVersion() {
        var pkg = CroniqRunner.class.getPackage();
        var v = pkg == null ? null : pkg.getImplementationVersion();
        return v != null ? v : "0.0.0-dev";
    }
}
