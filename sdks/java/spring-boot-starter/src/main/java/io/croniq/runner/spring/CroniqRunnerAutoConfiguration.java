package io.croniq.runner.spring;

import io.croniq.runner.CroniqRunner;
import io.croniq.runner.config.CroniqRunnerOptions;
import org.springframework.boot.autoconfigure.AutoConfiguration;
import org.springframework.boot.autoconfigure.condition.ConditionalOnMissingBean;
import org.springframework.boot.autoconfigure.condition.ConditionalOnProperty;
import org.springframework.boot.context.properties.EnableConfigurationProperties;
import org.springframework.context.ApplicationContext;
import org.springframework.context.annotation.Bean;

/**
 * Auto-configures a {@link CroniqRunner} from {@code croniq.runner.*}
 * properties and registers every {@link CroniqJob}-annotated method in the
 * context as a handler.
 *
 * <p>Disable with {@code croniq.runner.enabled=false}.
 */
@AutoConfiguration
@EnableConfigurationProperties(CroniqProperties.class)
@ConditionalOnProperty(prefix = "croniq.runner", name = "enabled", havingValue = "true", matchIfMissing = true)
public class CroniqRunnerAutoConfiguration {

    /**
     * Construct the {@link CroniqRunner} from {@link CroniqProperties} and
     * the {@code @CroniqJob}-annotated methods on every bean in the context.
     * Marked {@code @ConditionalOnMissingBean} so applications can supply
     * a pre-built runner (e.g., for testing) without disabling the starter.
     */
    @Bean
    @ConditionalOnMissingBean(CroniqRunnerOptions.class)
    public CroniqRunnerOptions croniqRunnerOptions(CroniqProperties properties) {
        return properties.toOptions();
    }

    @Bean
    @ConditionalOnMissingBean
    public CroniqRunner croniqRunner(CroniqRunnerOptions options, ApplicationContext context) {
        CroniqRunner.Builder builder = CroniqRunner.builder().options(options);
        CroniqJobScanner.registerAll(builder, context);
        return builder.build();
    }

    @Bean
    @ConditionalOnMissingBean
    public CroniqRunnerLifecycle croniqRunnerLifecycle(CroniqRunner runner) {
        return new CroniqRunnerLifecycle(runner);
    }
}
