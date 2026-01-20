import { Observable } from 'rxjs';

export type SseEvent = {
    data: string;
    event?: string;
    id?: string;
};

export type SseStreamOptions = {
    headers?: Record<string, string>;
    signal?: AbortSignal;
};

export function createSseStream(url: string, options: SseStreamOptions = {}): Observable<SseEvent> {
    return new Observable<SseEvent>((subscriber) => {
        const controller = new AbortController();
        const externalSignal = options.signal;
        const onAbort = () => controller.abort();

        if (externalSignal) {
            if (externalSignal.aborted) {
                controller.abort();
            } else {
                externalSignal.addEventListener('abort', onAbort, { once: true });
            }
        }

        const readStream = async (): Promise<void> => {
            try {
                const response = await fetch(url, {
                    headers: options.headers,
                    cache: 'no-store',
                    signal: controller.signal,
                });

                if (!response.ok) {
                    throw new Error(`SSE request failed with ${response.status} ${response.statusText}`);
                }

                const reader = response.body?.getReader();
                if (!reader) {
                    throw new Error('SSE response body is not readable.');
                }

                const decoder = new TextDecoder();
                let buffer = '';

                while (true) {
                    const { value, done } = await reader.read();
                    if (done) {
                        throw new Error('SSE stream closed unexpectedly.');
                    }

                    buffer += decoder.decode(value, { stream: true });
                    const parsed = splitSseEvents(buffer);
                    buffer = parsed.remainder;
                    parsed.events.forEach((event) => subscriber.next(event));
                }
            } catch (error) {
                if (!controller.signal.aborted) {
                    subscriber.error(error);
                }
            }
        };

        void readStream();

        return () => {
            if (externalSignal) {
                externalSignal.removeEventListener('abort', onAbort);
            }
            controller.abort();
        };
    });
}

export function splitSseEvents(buffer: string): { events: SseEvent[]; remainder: string } {
    const normalized = buffer.replace(/\r\n/g, '\n');
    const events: SseEvent[] = [];
    let offset = 0;

    while (true) {
        const boundary = normalized.indexOf('\n\n', offset);
        if (boundary < 0) {
            break;
        }

        const raw = normalized.slice(offset, boundary);
        const event = parseSseEvent(raw);
        if (event) {
            events.push(event);
        }
        offset = boundary + 2;
    }

    return {
        events,
        remainder: normalized.slice(offset),
    };
}

function parseSseEvent(raw: string): SseEvent | null {
    if (!raw.trim()) {
        return null;
    }

    let data = '';
    let event: string | undefined;
    let id: string | undefined;

    raw.split('\n').forEach((line) => {
        if (!line || line.startsWith(':')) {
            return;
        }

        const separatorIndex = line.indexOf(':');
        const field = separatorIndex >= 0 ? line.slice(0, separatorIndex) : line;
        const value = separatorIndex >= 0 ? line.slice(separatorIndex + 1).trimStart() : '';

        if (field === 'data') {
            data = data ? `${data}\n${value}` : value;
            return;
        }
        if (field === 'event') {
            event = value;
            return;
        }
        if (field === 'id') {
            id = value;
        }
    });

    if (!data && !event && !id) {
        return null;
    }

    return {
        data,
        event,
        id,
    };
}
