/**
 * Wire-protocol DTOs. Mirrors {@code openapi.yaml} verbatim — snake_case
 * field names on the wire; camelCase record components annotated with
 * {@link com.fasterxml.jackson.annotation.JsonProperty}.
 *
 * <p>Equivalent to the .NET SDK's {@code Croniq.Runner.Sdk.Protocol} namespace.
 * When the wire protocol changes, the case is added to
 * {@code sdks/conformance/cases/} first, then the DTO follows.
 */
package io.croniq.runner.protocol;
