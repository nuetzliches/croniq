import { DestroyRef, type WritableSignal, effect, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { ActivatedRoute, Router } from '@angular/router';

type SelectionNormalizer = (value: string | number | null) => string | number | null;

type SelectionSyncOptions = {
    paramKey: string;
    normalize?: SelectionNormalizer;
};

const defaultNormalize: SelectionNormalizer = (value) => {
    if (value === null || value === undefined) {
        return null;
    }
    if (typeof value === 'number' && Number.isFinite(value)) {
        return value;
    }
    const normalized = String(value).trim();
    return normalized.length > 0 ? normalized : null;
};

export function bindQueryParam(options: SelectionSyncOptions): WritableSignal<string | number | null> {
    const route = inject(ActivatedRoute);
    const router = inject(Router);
    const destroyRef = inject(DestroyRef);
    const normalize = options.normalize ?? defaultNormalize;
    const selectedId = signal<string | number | null>(null);
    const lastSynced = signal<string | number | null>(null);

    route.queryParamMap
        .pipe(takeUntilDestroyed(destroyRef))
        .subscribe((params) => {
            const raw = params.get(options.paramKey);
            const normalized = normalize(raw ?? null);
            if (selectedId() !== normalized) {
                selectedId.set(normalized);
            }
            if (lastSynced() !== normalized) {
                lastSynced.set(normalized);
            }
        });

    effect(() => {
        const current = normalize(selectedId());
        if (current !== selectedId()) {
            selectedId.set(current);
        }
        if (current === lastSynced()) {
            return;
        }
        lastSynced.set(current);
        void router.navigate([], {
            relativeTo: route,
            queryParams: { [options.paramKey]: current },
            queryParamsHandling: 'merge',
            replaceUrl: true,
        });
    });

    return selectedId as WritableSignal<string | number | null>;
}
