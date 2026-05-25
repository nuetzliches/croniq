// Shared Java-module config: toolchain, formatter, lint, test framework.
// Applied by every module that compiles Java (core, spring-boot-starter,
// conformance-tests). The Kotlin module applies croniq.kotlin-conventions
// instead, which layers on top of this.

plugins {
    `java-library`
    checkstyle
    id("com.diffplug.spotless")
}

// Project requires JDK 21+: virtual threads (Project Loom) are the
// concurrency primitive used by the runner's poll loop and per-execution
// handlers. Earlier JDKs would force a parallel implementation.
java {
    toolchain {
        languageVersion.set(JavaLanguageVersion.of(21))
    }
    withSourcesJar()
    withJavadocJar()
}

tasks.withType<JavaCompile>().configureEach {
    options.encoding = "UTF-8"
    options.release.set(21)
    // -parameters keeps method parameter names in the bytecode. Required
    // for Spring's @ConfigurationProperties record binding and useful for
    // anyone reflecting over our public API.
    // -Xlint:-processing excludes the "annotation was not claimed by any
    // processor" warning. Spring Boot's @Bean / @AutoConfiguration etc. are
    // processed at runtime, not at compile time, but javac doesn't know that
    // and warns. With -Werror set, the warning would fail the build.
    options.compilerArgs.addAll(listOf("-parameters", "-Xlint:all,-processing", "-Werror"))
}

tasks.withType<Javadoc>().configureEach {
    // Tolerate modules with no public types yet (PR-1 stubs, package-info-only
    // modules). The javadoc tool errors out on empty source by default; we
    // want the build to succeed and produce an empty javadoc.jar — Maven
    // Central rejects publication without one but doesn't validate contents.
    isFailOnError = false
    (options as StandardJavadocDocletOptions).apply {
        addStringOption("Xdoclint:none", "-quiet")
        encoding = "UTF-8"
        charSet = "UTF-8"
        // Link against the JDK 21 module docs so {@link} works for
        // java.net.http.HttpClient, java.util.concurrent.Flow, etc.
        links("https://docs.oracle.com/en/java/javase/21/docs/api/")
    }
}

testing {
    suites {
        val test by getting(JvmTestSuite::class) {
            useJUnitJupiter()
            dependencies {
                // Versions come from the root project's catalog via the
                // `libs` extension generated from gradle/libs.versions.toml.
                implementation(project())
            }
        }
    }
}

tasks.named<Test>("test") {
    // Surface failures fast; verbose output is captured in the upload step.
    testLogging {
        events("passed", "skipped", "failed")
        exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
        showStandardStreams = false
    }
    // JUnit 5 supports parallel test execution out of the box. The unit
    // tests don't touch shared state; conformance-tests override this back
    // to single-threaded because the mock HTTP server is shared per case.
    systemProperty("junit.jupiter.execution.parallel.enabled", "true")
    systemProperty("junit.jupiter.execution.parallel.mode.default", "concurrent")
}

checkstyle {
    toolVersion = "10.20.1"
    configFile = rootProject.file("config/checkstyle/checkstyle.xml")
    // Treat warnings as errors so style violations actually block the build.
    // Without this, checkstyle reports issues but the task succeeds.
    maxWarnings = 0
    isIgnoreFailures = false
}

// Checkstyle complains about generated/test sources for low ROI. Keep
// it focused on main source — tests are still formatted via Spotless.
tasks.named("checkstyleTest") {
    enabled = false
}

spotless {
    // Force LF everywhere — Windows runners check out with CRLF by default
    // and ktlint's line-ending check fails the build before the formatter
    // even runs. The repo .gitattributes also pins LF; double-belted.
    lineEndings = com.diffplug.spotless.LineEnding.UNIX

    java {
        target("src/**/*.java")
        palantirJavaFormat("2.50.0")
        removeUnusedImports()
        trimTrailingWhitespace()
        endWithNewline()
        // License header omitted intentionally — file-level headers create
        // diff noise on every touch and the repo-root LICENSE-MIT /
        // LICENSE-APACHE files are the canonical statement.
    }
    kotlinGradle {
        target("*.gradle.kts")
        ktlint("1.4.1")
    }
}
