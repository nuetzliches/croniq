import { inject, type ResourceRef } from '@angular/core';
import { rxResource } from '@angular/core/rxjs-interop';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import type { CallerContext, CroniqRequestOptions } from 'data-access';
import type { Observable } from 'rxjs';

export type TenantRxResourceStreamArgs<R> = {
    params: R;
    requestOptions: CroniqRequestOptions;
};

export interface TenantRxResourceOptions<T, R> {
    command: string;
    defaultValue: T;
    params: () => R;
    callerContextOverrides?: (params: R) => Partial<CallerContext>;
    stream: (args: TenantRxResourceStreamArgs<R>) => Observable<T>;
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
        stream: ({ params }) =>
            opts.stream({
                params,
                requestOptions: tenantContext.createRequestOptions(
                    opts.command,
                    opts.callerContextOverrides ? opts.callerContextOverrides(params) : undefined,
                ),
            }),
    });
}
