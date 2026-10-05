package io.croniq.runner;

import org.junit.platform.launcher.LauncherSession;
import org.junit.platform.launcher.LauncherSessionListener;
import org.slf4j.LoggerFactory;

/**
 * Binds SLF4J to logback on one thread before any test class is loaded (#807).
 *
 * <p>This module's test classes run concurrently. Whichever thread first calls
 * {@link LoggerFactory} performs SLF4J's initialisation, and a class initialised on another
 * thread meanwhile gets a {@code SubstituteLogger} in its {@code static final Logger} field.
 * SLF4J 2.0.16 publishes "initialised" before it hands those substitutes their delegate, and a
 * substitute requested after it has cleared its list never gets one: it logs to
 * {@code NOPLogger} for the life of the JVM. {@code ServerUrls} holds such a field, so
 * {@code ServerUrlsTest}'s warning assertions intermittently saw an empty appender.
 *
 * <p>Registered in {@code META-INF/services/org.junit.platform.launcher.LauncherSessionListener}.
 */
public final class BindSlf4jBeforeTests implements LauncherSessionListener {

    @Override
    public void launcherSessionOpened(LauncherSession session) {
        // Returns only after the binding is complete, substitutes included.
        LoggerFactory.getILoggerFactory();
    }
}
