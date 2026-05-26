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
 *   <li>{@link io.croniq.runner.config} — runner options and configuration records.
 *   <li>{@link io.croniq.runner.protocol} — wire-protocol DTOs derived from {@code openapi.yaml}.
 *   <li>{@link io.croniq.runner.handler} — handler interface, registration, dispatcher.
 *   <li>{@link io.croniq.runner.logging} — streaming log writer plumbing.
 *   <li>{@link io.croniq.runner.internal} — package-private transport, scheduling, identity.
 *   <li>{@link io.croniq.runner.shell} — DSL {@code runner shell {…}} / {@code runner exec {…}} decoder.
 * </ul>
 *
 * <p>Wire-protocol fidelity is asserted by the shared conformance suite at
 * {@code sdks/conformance/cases/}, run against this SDK by the
 * {@code conformance-tests} module.
 */
package io.croniq.runner;
