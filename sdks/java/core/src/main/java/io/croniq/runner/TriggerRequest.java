package io.croniq.runner;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;
import java.util.List;
import java.util.Map;
import java.util.Objects;

/**
 * A single on-demand job trigger ({@code POST /v1/trigger}).
 *
 * <p>Build via {@link #builder(String)} — only {@code jobKey} is required.
 * Unset optional fields are {@code null} and are omitted from the JSON body
 * (never sent as {@code null}) via {@link JsonInclude.Include#NON_NULL}, so a
 * producer never emits {@code metadata} / {@code require} / {@code prefer} /
 * {@code timeout} / {@code idempotencyKey} it was not given.
 *
 * @param jobKey job key, e.g. {@code billing:invoice-generate}. Required.
 * @param metadata metadata forwarded to the handler as arbitrary JSON, merged
 *     over the job's DSL metadata. Values may be nested objects / arrays /
 *     numbers / booleans, not just strings; keys starting with {@code __} are
 *     reserved for internal use.
 * @param require capabilities a runner MUST have to be assigned this execution.
 * @param prefer capabilities used to prefer runners when several are eligible.
 * @param timeout execution timeout as a server duration string (e.g.
 *     {@code "30s"}, {@code "5m"}); the server default applies when {@code null}.
 * @param idempotencyKey optional dedup key. Servers with trigger-idempotency
 *     support coalesce repeat triggers carrying the same key onto the existing
 *     execution (see {@link TriggerResult#deduplicated()}); older servers ignore
 *     it. Capped server-side at 200 characters — a longer key is rejected.
 */
@JsonInclude(JsonInclude.Include.NON_NULL)
public record TriggerRequest(
        @JsonProperty("job_key") String jobKey,
        @JsonProperty("metadata") Map<String, Object> metadata,
        @JsonProperty("require") List<String> require,
        @JsonProperty("prefer") List<String> prefer,
        @JsonProperty("timeout") String timeout,
        @JsonProperty("idempotency_key") String idempotencyKey) {

    public TriggerRequest {
        Objects.requireNonNull(jobKey, "jobKey");
    }

    /** Start building a trigger for {@code jobKey}. */
    public static Builder builder(String jobKey) {
        return new Builder(jobKey);
    }

    public static final class Builder {
        private final String jobKey;
        private Map<String, Object> metadata;
        private List<String> require;
        private List<String> prefer;
        private String timeout;
        private String idempotencyKey;

        private Builder(String jobKey) {
            this.jobKey = jobKey;
        }

        public Builder metadata(Map<String, Object> v) {
            this.metadata = v;
            return this;
        }

        public Builder require(List<String> v) {
            this.require = v;
            return this;
        }

        public Builder prefer(List<String> v) {
            this.prefer = v;
            return this;
        }

        public Builder timeout(String v) {
            this.timeout = v;
            return this;
        }

        public Builder idempotencyKey(String v) {
            this.idempotencyKey = v;
            return this;
        }

        public TriggerRequest build() {
            return new TriggerRequest(jobKey, metadata, require, prefer, timeout, idempotencyKey);
        }
    }
}
