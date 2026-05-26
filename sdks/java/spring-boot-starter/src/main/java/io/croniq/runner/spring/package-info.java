/**
 * Spring Boot starter for the Croniq Runner SDK.
 *
 * <p>Opt-in auto-configuration that binds {@code croniq.runner.*} properties,
 * scans for {@code @CroniqJob}-annotated beans, and registers them with the
 * core {@code CroniqRunner}. Implementation lands in PR-5 of issue #133.
 *
 * <p>Consumers who don't use Spring should depend on
 * {@code io.github.nuetzliches:croniq-runner} directly and avoid this artifact.
 */
package io.croniq.runner.spring;
