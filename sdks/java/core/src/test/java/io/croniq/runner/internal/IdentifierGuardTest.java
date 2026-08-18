package io.croniq.runner.internal;

import static org.assertj.core.api.Assertions.assertThat;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.ValueSource;

/** Ingest validation and log hygiene for server-supplied identifiers (#441). */
class IdentifierGuardTest {

    private static final String ESC = "\u001b";
    private static final String CRLF_KEY = "billing:invoice\r\n2026-01-01 ERROR forged record";
    private static final String ANSI_KEY = "billing:" + ESC + "[31minvoice" + ESC + "[0m";

    @ParameterizedTest
    @ValueSource(
            strings = {
                "billing:invoice",
                "ops:health:eu-west",
                "ops:db-dump",
                "a:b",
                "ns:name.with.dots",
                "ns:name_with_underscore",
                "ns:path/segment",
                "ns:*",
                "ns:name+variant@host",
                "ns:what?"
            })
    void acceptsEveryKeyTheLexerCanProduceUnquoted(String key) {
        assertThat(IdentifierGuard.isSafeJobKey(key)).isTrue();
    }

    /**
     * {@code job "billing:monthly invoice" { … }} is legal DSL: parse_job_key
     * accepts a QuotedString and enforces only the colon-part count, and
     * POST /v1/jobs constrains the key not at all. An allowlist would strand
     * these valid configurations, so interior spaces and non-ASCII text pass.
     */
    @ParameterizedTest
    @ValueSource(
            strings = {
                "billing:monthly invoice",
                "ops:health check:eu-west",
                "berichte:monatsabschluss (märz)",
                "ops:1С-выгрузка",
                "ops:日次バッチ",
                "ops:deploy#42",
                "ops:a,b;c",
                "ops:100%-check",
                "ops:emoji-🚀",
                // A trailing or interior space cannot forge a record.
                "billing:invoice ",
                "billing: invoice"
            })
    void acceptsQuotedAndNonAsciiKeys(String key) {
        assertThat(IdentifierGuard.isSafeJobKey(key)).isTrue();
    }

    @Test
    void rejectsControlCharactersInJobKeys() {
        assertThat(IdentifierGuard.isSafeJobKey(CRLF_KEY)).isFalse();
        assertThat(IdentifierGuard.isSafeJobKey(ANSI_KEY)).isFalse();
        assertThat(IdentifierGuard.isSafeJobKey("billing:in\u0000voice")).isFalse();
        assertThat(IdentifierGuard.isSafeJobKey("billing:in\tvoice")).isFalse();
        assertThat(IdentifierGuard.isSafeJobKey("billing:invoice\u007f")).isFalse();
        assertThat(IdentifierGuard.isSafeJobKey("billing:invoice\u009b")).isFalse();
        assertThat(IdentifierGuard.isSafeJobKey("")).isFalse();
        assertThat(IdentifierGuard.isSafeJobKey(null)).isFalse();
    }

    @Test
    void jobKeyLengthBoundIsInclusive() {
        assertThat(IdentifierGuard.isSafeJobKey("a".repeat(IdentifierGuard.MAX_JOB_KEY_LENGTH)))
                .isTrue();
        assertThat(IdentifierGuard.isSafeJobKey("a".repeat(IdentifierGuard.MAX_JOB_KEY_LENGTH + 1)))
                .isFalse();
    }

    /**
     * The bound counts code points, not {@code char} units: a supplementary
     * character is two units but one character, so a key of MAX_JOB_KEY_LENGTH
     * emoji must pass rather than be rejected at half its logical length.
     */
    @Test
    void jobKeyLengthBoundCountsCodePoints() {
        assertThat(IdentifierGuard.isSafeJobKey("🚀".repeat(IdentifierGuard.MAX_JOB_KEY_LENGTH)))
                .isTrue();
        assertThat(IdentifierGuard.isSafeJobKey("🚀".repeat(IdentifierGuard.MAX_JOB_KEY_LENGTH + 1)))
                .isFalse();
    }

    @Test
    void executionIdAcceptsUuidAndOpaqueIds() {
        assertThat(IdentifierGuard.isSafeExecutionId("6f8c1a2e-4b7d-4a1f-9c3e-2d5b8a0f1e77"))
                .isTrue();
        assertThat(IdentifierGuard.isSafeExecutionId("exec-001")).isTrue();
    }

    @Test
    void executionIdRejectsHostileAndOutOfBound() {
        assertThat(IdentifierGuard.isSafeExecutionId("exec-001\r\nforged")).isFalse();
        assertThat(IdentifierGuard.isSafeExecutionId("exec" + ESC + "[2J001")).isFalse();
        assertThat(IdentifierGuard.isSafeExecutionId("")).isFalse();
        assertThat(IdentifierGuard.isSafeExecutionId("a".repeat(IdentifierGuard.MAX_EXECUTION_ID_LENGTH + 1)))
                .isFalse();
    }

    @Test
    void rejectAssignmentReasonNamesTheField() {
        assertThat(IdentifierGuard.rejectAssignmentReason("exec-001", "billing:invoice"))
                .isNull();
        assertThat(IdentifierGuard.rejectAssignmentReason("exec-001", "billing:monthly invoice"))
                .isNull();
        assertThat(IdentifierGuard.rejectAssignmentReason("exec-001", CRLF_KEY)).isEqualTo("job_key");
        assertThat(IdentifierGuard.rejectAssignmentReason("exec\r\n001", "billing:invoice"))
                .isEqualTo("execution_id");
        // execution_id is checked first: it is what addresses the server, so
        // when both are bad the assignment is unackable and must be dropped.
        assertThat(IdentifierGuard.rejectAssignmentReason("exec\r\n001", CRLF_KEY))
                .isEqualTo("execution_id");
    }

    @Test
    void rejectionAckErrorNamesTheFieldAndEscapesTheValue() {
        String message = IdentifierGuard.rejectionAckError("job_key", CRLF_KEY);
        assertThat(message).contains("job_key");
        assertThat(message).doesNotContain("\r");
        assertThat(message).doesNotContain("\n");
        assertThat(message).contains("\\u000d\\u000a");
    }

    @Test
    void escapeControlCharsCoversC0EscAndC1() {
        assertThat(IdentifierGuard.escapeControlChars("a\r\nb")).isEqualTo("a\\u000d\\u000ab");
        assertThat(IdentifierGuard.escapeControlChars(ESC + "[31mred")).isEqualTo("\\u001b[31mred");
        assertThat(IdentifierGuard.escapeControlChars("\u009b")).isEqualTo("\\u009b");
        assertThat(IdentifierGuard.escapeControlChars("billing:invoice — läuft"))
                .isEqualTo("billing:invoice — läuft");
    }

    @Test
    void previewForLogEscapesAndTruncates() {
        assertThat(IdentifierGuard.previewForLog(CRLF_KEY)).doesNotContain("\n");
        assertThat(IdentifierGuard.previewForLog(ANSI_KEY)).doesNotContain(ESC);
        assertThat(IdentifierGuard.previewForLog("a".repeat(500)).length()).isLessThanOrEqualTo(121);
    }
}
