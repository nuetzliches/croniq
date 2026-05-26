package io.croniq.runner.handler;

/**
 * Wraps any exception thrown by a handler so the dispatcher can distinguish
 * handler-originating failures from transport / framework errors when shaping
 * the ack payload. Handlers SHOULD throw their own exception types; the SDK
 * wraps them with this on the dispatcher edge.
 */
public class CroniqHandlerException extends RuntimeException {
    private static final long serialVersionUID = 1L;

    public CroniqHandlerException(String message, Throwable cause) {
        super(message, cause);
    }
}
