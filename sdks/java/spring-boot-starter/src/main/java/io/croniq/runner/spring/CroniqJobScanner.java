package io.croniq.runner.spring;

import io.croniq.runner.CroniqRunner;
import io.croniq.runner.handler.CroniqExecutionContext;
import io.croniq.runner.handler.CroniqJobHandler;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.context.ApplicationContext;
import org.springframework.core.MethodIntrospector;
import org.springframework.core.annotation.AnnotationUtils;

/**
 * Scans the {@link ApplicationContext} for {@code @CroniqJob}-annotated
 * methods and registers each as a handler on the supplied
 * {@link CroniqRunner.Builder}. Mirrors Spring's
 * {@code ScheduledAnnotationBeanPostProcessor} but with a single-pass scan at
 * builder time — no proxies, no AOP, no per-bean post-processing overhead.
 */
final class CroniqJobScanner {

    private static final Logger log = LoggerFactory.getLogger(CroniqJobScanner.class);

    private CroniqJobScanner() {}

    static void registerAll(CroniqRunner.Builder builder, ApplicationContext context) {
        for (String beanName : context.getBeanDefinitionNames()) {
            Object bean;
            try {
                bean = context.getBean(beanName);
            } catch (Exception e) {
                // Skip beans we can't materialise (FactoryBeans for other
                // unsatisfied dependencies, …). They can't host job methods
                // anyway.
                continue;
            }
            Class<?> target = bean.getClass();
            var methods = MethodIntrospector.selectMethods(
                    target, (Method m) -> AnnotationUtils.findAnnotation(m, CroniqJob.class) != null);
            for (Method method : methods) {
                CroniqJob annotation = AnnotationUtils.findAnnotation(method, CroniqJob.class);
                if (annotation == null) {
                    continue;
                }
                validateSignature(method);
                CroniqJobHandler handler = adapt(bean, method);
                if (annotation.schedule().isBlank()) {
                    builder.addJob(annotation.key(), handler);
                } else {
                    builder.addJob(annotation.key(), annotation.schedule(), handler);
                }
                log.info(
                        "Registered Croniq job: key={} bean={}.{}",
                        annotation.key(),
                        target.getSimpleName(),
                        method.getName());
            }
        }
    }

    private static void validateSignature(Method m) {
        Class<?>[] params = m.getParameterTypes();
        if (params.length != 1 || !params[0].isAssignableFrom(CroniqExecutionContext.class)) {
            throw new IllegalStateException(
                    "@CroniqJob method " + m.getDeclaringClass().getSimpleName() + "#" + m.getName()
                            + " must take exactly one CroniqExecutionContext parameter");
        }
    }

    private static CroniqJobHandler adapt(Object bean, Method method) {
        if (!method.canAccess(bean)) {
            method.setAccessible(true);
        }
        return ctx -> {
            try {
                method.invoke(bean, ctx);
            } catch (InvocationTargetException ite) {
                Throwable cause = ite.getCause();
                if (cause instanceof Exception e) {
                    throw e;
                }
                throw new RuntimeException(cause);
            }
        };
    }
}
