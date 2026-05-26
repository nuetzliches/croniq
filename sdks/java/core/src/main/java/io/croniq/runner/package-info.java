/**
 * Croniq Runner SDK for Java — root package.
 *
 * <p>This module is the canonical, library-not-framework Java SDK for building
 * Croniq runners. It polls a Croniq server for work, dispatches typed
 * handlers, streams structured logs back, and reports completion.
 *
 * <p>Sub-packages follow the same layout as the reference .NET SDK at
 * {@code sdks/dotnet/src/Croniq.Runner.Sdk/}:
 *
 * <ul>
 *   <li>{@link io.croniq.runner.config} — runner options.
 *   <li>{@link io.croniq.runner.protocol} — wire-protocol DTOs derived from {@code openapi.yaml}.
 *   <li>{@link io.croniq.runner.handler} — handler interface, execution context.
 *   <li>{@link io.croniq.runner.internal} — package-private transport, scheduling, identity.
 * </ul>
 *
 * <p>Future packages ({@code logging}, {@code shell}) land in later PRs of
 * issue #133.
 *
 * <p>Wire-protocol fidelity is asserted by the shared conformance suite at
 * {@code sdks/conformance/cases/}, run against this SDK by the
 * {@code conformance-tests} module.
 */
package io.croniq.runner;
