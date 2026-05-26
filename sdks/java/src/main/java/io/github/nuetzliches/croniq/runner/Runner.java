package io.github.nuetzliches.croniq.runner;

/**
 * Placeholder for the Croniq runner. Real implementation tracked in
 * <a href="https://github.com/nuetzliches/croniq/issues/133">issue #133</a>.
 *
 * <p>This class exists so the 0.0.x line can publish to Maven Central as a
 * smoke test of the release pipeline (build → sign → upload). It will be
 * replaced by the actual runner API in the first feature PR.
 */
public final class Runner {

    private Runner() {
        // Intentionally non-instantiable. The public API lands with issue #133.
    }

    /**
     * @return the SDK version, derived from the published Maven coordinates.
     */
    public static String version() {
        var pkg = Runner.class.getPackage();
        var v = pkg != null ? pkg.getImplementationVersion() : null;
        return v != null ? v : "dev";
    }
}
