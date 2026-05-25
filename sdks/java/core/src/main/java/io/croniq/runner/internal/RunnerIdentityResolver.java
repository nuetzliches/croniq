package io.croniq.runner.internal;

import io.croniq.runner.config.CroniqRunnerOptions;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.SecureRandom;
import java.util.HexFormat;
import java.util.Locale;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Resolves the runner_id. Precedence (matches the .NET SDK):
 *
 * <ol>
 *   <li>{@code options.runnerId()} if non-blank.
 *   <li>{@code RUNNER_ID} environment variable.
 *   <li>Persisted file at {@code <runnerDataDir>/runner-id}.
 *   <li>Generate {@code <prefix>-<8 hex chars>}, persist to the file for
 *       stability across restarts.
 * </ol>
 *
 * <p>If the data directory is unwritable, falls back to
 * {@code <prefix>-<machine-name>} so the runner can still start.
 */
public final class RunnerIdentityResolver {

    private static final Logger log = LoggerFactory.getLogger(RunnerIdentityResolver.class);
    private static final String STATE_FILE = "runner-id";
    private static final String ENV_VAR = "RUNNER_ID";
    private static final String ENV_DATA_DIR = "CRONIQ_RUNNER_DATA_DIR";

    private final CroniqRunnerOptions options;
    private final SecureRandom random;

    public RunnerIdentityResolver(CroniqRunnerOptions options) {
        this(options, new SecureRandom());
    }

    RunnerIdentityResolver(CroniqRunnerOptions options, SecureRandom random) {
        this.options = options;
        this.random = random;
    }

    public String resolve() {
        if (options.runnerId() != null && !options.runnerId().isBlank()) {
            return options.runnerId();
        }
        String env = System.getenv(ENV_VAR);
        if (env != null && !env.isBlank()) {
            return env.trim();
        }

        Path dataDir = resolveDataDir();
        if (dataDir != null) {
            Path file = dataDir.resolve(STATE_FILE);
            try {
                if (Files.isRegularFile(file)) {
                    String persisted =
                            Files.readString(file, StandardCharsets.UTF_8).trim();
                    if (!persisted.isBlank()) {
                        return persisted;
                    }
                }
                String generated = generate(options.runnerIdPrefix());
                Files.createDirectories(dataDir);
                Files.writeString(file, generated, StandardCharsets.UTF_8);
                return generated;
            } catch (IOException e) {
                log.debug("Could not persist runner-id under {} — falling back to hostname", dataDir, e);
            }
        }

        return options.runnerIdPrefix() + "-" + hostnameSlug();
    }

    private Path resolveDataDir() {
        if (options.runnerDataDir() != null && !options.runnerDataDir().isBlank()) {
            return Path.of(options.runnerDataDir());
        }
        String env = System.getenv(ENV_DATA_DIR);
        if (env != null && !env.isBlank()) {
            return Path.of(env);
        }
        // Platform default — mirrors the .NET SDK's LocalApplicationData fallback.
        String xdg = System.getenv("XDG_DATA_HOME");
        if (xdg != null && !xdg.isBlank()) {
            return Path.of(xdg, "croniq-runner");
        }
        String home = System.getProperty("user.home");
        if (home != null && !home.isBlank()) {
            return Path.of(home, ".local", "share", "croniq-runner");
        }
        return null;
    }

    private String generate(String prefix) {
        byte[] buf = new byte[4];
        random.nextBytes(buf);
        return prefix + "-" + HexFormat.of().formatHex(buf);
    }

    private static String hostnameSlug() {
        String name = System.getenv("HOSTNAME");
        if (name == null || name.isBlank()) {
            try {
                name = java.net.InetAddress.getLocalHost().getHostName();
            } catch (Exception e) {
                name = "unknown";
            }
        }
        // Slug — strip anything that's not lowercase / digits / hyphen.
        return name.toLowerCase(Locale.ROOT).replaceAll("[^a-z0-9-]", "-");
    }
}
