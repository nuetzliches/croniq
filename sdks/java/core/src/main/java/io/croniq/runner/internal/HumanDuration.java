package io.croniq.runner.internal;

import java.time.Duration;
import java.util.Locale;
import java.util.regex.Pattern;

/**
 * Parses humane duration strings used on the wire — {@code "30s"}, {@code "5m"},
 * {@code "1h30m"}, etc. Matches the .NET SDK's parser and the Croniqfile DSL
 * conventions.
 *
 * <p>Accepts: a sequence of {@code <integer><unit>} pairs where unit is one of
 * {@code s | m | h | d}. Bare integers default to seconds.
 */
public final class HumanDuration {

    private static final Pattern PAIR = Pattern.compile("(\\d+)([smhd])");

    private HumanDuration() {}

    public static Duration parse(String s) {
        if (s == null || s.isBlank()) {
            return Duration.ZERO;
        }
        String norm = s.trim().toLowerCase(Locale.ROOT);
        // Bare integer: treat as seconds. Mirrors the server-side parser.
        try {
            long secs = Long.parseLong(norm);
            return Duration.ofSeconds(secs);
        } catch (NumberFormatException ignored) {
            // fall through to unit-pair parsing
        }
        var m = PAIR.matcher(norm);
        Duration acc = Duration.ZERO;
        int end = 0;
        while (m.find()) {
            if (m.start() != end) {
                throw new IllegalArgumentException("Unparseable duration: '" + s + "'");
            }
            end = m.end();
            long n = Long.parseLong(m.group(1));
            char unit = m.group(2).charAt(0);
            acc = acc.plus(
                    switch (unit) {
                        case 's' -> Duration.ofSeconds(n);
                        case 'm' -> Duration.ofMinutes(n);
                        case 'h' -> Duration.ofHours(n);
                        case 'd' -> Duration.ofDays(n);
                        default -> throw new IllegalArgumentException("unit: " + unit);
                    });
        }
        if (end != norm.length() || acc.isZero()) {
            throw new IllegalArgumentException("Unparseable duration: '" + s + "'");
        }
        return acc;
    }
}
