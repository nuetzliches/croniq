import { inject, type ResourceRef, type ResourceStatus } from '@angular/core';
import { rxResource } from '@angular/core/rxjs-interop';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import type { CallerContext, CroniqRequestOptions } from 'data-access';
import { of, tap, type Observable } from 'rxjs';

export type TenantRxResourceStreamArgs<R> = {
    params: R;
    requestOptions: CroniqRequestOptions;
    abortSignal: AbortSignal;
    previousStatus: ResourceStatus;
};

export type TenantRxResourceCacheArgs<R> = {
    params: R;
    previousStatus: ResourceStatus;
};

export type TenantRxResourceCacheReadResult<T> =
    | { hit: true; value: T }
    | { hit: false };

export type TenantRxResourceCache<T, R> = {
    key: (params: R) => string | null;
    read: (key: string) => TenantRxResourceCacheReadResult<T>;
    write: (key: string, value: T) => void;
    shouldUse?: (value: T, args: TenantRxResourceCacheArgs<R>) => boolean;
    shouldStore?: (value: T, args: TenantRxResourceCacheArgs<R>) => boolean;
};

export interface TenantRxResourceOptions<T, R> {
    command: string;
    defaultValue: T;
    params: () => R;
    callerContextOverrides?: (params: R) => Partial<CallerContext>;
    stream: (args: TenantRxResourceStreamArgs<R>) => Observable<T>;
    cache?: TenantRxResourceCache<T, R>;
}

/**
 * Small helper around rxResource that wires `CroniqRequestOptions` (caller context) from TenantContext.
 *
 * - No external query libs; uses Angular v21 resources only.
 * - `params` drives re-fetching; `reload()` can be used for manual refresh.
 */
export function tenantRxResource<T, R>(opts: TenantRxResourceOptions<T, R>): ResourceRef<T> {
    const tenantContext = inject(TenantContextService);

    return rxResource<T, R>({
        defaultValue: opts.defaultValue,
        params: opts.params,
        stream: ({ params, abortSignal, previous }) => {
            const cache = opts.cache;
            const cacheKey = cache ? cache.key(params) : null;
            const cacheArgs: TenantRxResourceCacheArgs<R> = {
                params,
                previousStatus: previous.status,
            };

            if (cache && cacheKey) {
                const cached = cache.read(cacheKey);
                if (cached.hit && (cache.shouldUse?.(cached.value, cacheArgs) ?? true)) {
                    return of(cached.value);
                }
            }

            return opts
                .stream({
                    params,
                    requestOptions: tenantContext.createRequestOptions(
                        opts.command,
                        opts.callerContextOverrides ? opts.callerContextOverrides(params) : undefined,
                    ),
                    abortSignal,
                    previousStatus: previous.status,
                })
                .pipe(
                    tap((value) => {
                        if (!cache || !cacheKey) {
                            return;
                        }
                        if (cache.shouldStore && !cache.shouldStore(value, cacheArgs)) {
                            return;
                        }
                        cache.write(cacheKey, value);
                    }),
                );
        },
    });
}
