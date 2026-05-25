package io.croniq.runner.spring;

import static org.assertj.core.api.Assertions.assertThat;

import io.croniq.runner.CroniqRunner;
import io.croniq.runner.handler.CroniqExecutionContext;
import org.junit.jupiter.api.Test;
import org.springframework.boot.autoconfigure.AutoConfigurations;
import org.springframework.boot.test.context.runner.ApplicationContextRunner;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;

class CroniqRunnerAutoConfigurationTest {

    private final ApplicationContextRunner contextRunner = new ApplicationContextRunner()
            .withConfiguration(AutoConfigurations.of(CroniqRunnerAutoConfiguration.class));

    @Test
    void disabledFlagSkipsAutoConfig() {
        contextRunner.withPropertyValues("croniq.runner.enabled=false").run(ctx -> assertThat(ctx)
                .doesNotHaveBean(CroniqRunner.class));
    }

    @Test
    void wiresRunnerWithPropertyBoundOptions() {
        contextRunner
                .withPropertyValues(
                        "croniq.runner.server-url=https://example.test",
                        "croniq.runner.api-key=ak-test",
                        "croniq.runner.capabilities[0]=billing",
                        "croniq.runner.tags[0]=env=test")
                .run(ctx -> {
                    assertThat(ctx).hasSingleBean(CroniqRunner.class);
                    assertThat(ctx).hasSingleBean(CroniqRunnerLifecycle.class);
                    var props = ctx.getBean(CroniqProperties.class);
                    assertThat(props.getServerUrl()).isEqualTo("https://example.test");
                    assertThat(props.getCapabilities()).containsExactly("billing");
                    assertThat(props.getTags()).containsExactly("env=test");
                });
    }

    @Test
    void scansCroniqJobAnnotatedMethods() {
        contextRunner.withUserConfiguration(JobBeanConfig.class).run(ctx -> {
            // Construction of the runner exercises CroniqJobScanner.registerAll —
            // duplicate registrations would have thrown. The scanner logs on
            // success and we just need to confirm the wiring path completes.
            assertThat(ctx).hasSingleBean(CroniqRunner.class);
        });
    }

    @Configuration
    static class JobBeanConfig {

        @Bean
        BillingJobs billingJobs() {
            return new BillingJobs();
        }
    }

    static class BillingJobs {

        @CroniqJob(key = "billing:invoice", schedule = "5m")
        public void handleInvoice(CroniqExecutionContext ctx) {
            // no-op
        }
    }
}
