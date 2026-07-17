package io.croniq.runner.internal;

import static org.assertj.core.api.Assertions.assertThat;

import java.time.Instant;
import org.junit.jupiter.api.Test;

class ScheduledForTest {

    @Test
    void parsesRfc3339() {
        assertThat(ExecutionDispatcher.parseScheduledFor("2026-06-01T06:00:00Z"))
                .isEqualTo(Instant.parse("2026-06-01T06:00:00Z"));
    }

    @Test
    void absentIsNull() {
        assertThat(ExecutionDispatcher.parseScheduledFor(null)).isNull();
        assertThat(ExecutionDispatcher.parseScheduledFor("")).isNull();
    }

    @Test
    void unparseableIsNullNotFireAt() {
        assertThat(ExecutionDispatcher.parseScheduledFor("not-a-date")).isNull();
    }
}
