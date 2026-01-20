import { ChangeDetectionStrategy, Component, DestroyRef, computed, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { OidcAuthService } from '@core/auth/oidc-auth.service';
import { RuntimeConfigService } from '@core/runtime-config.service';
import { AppBrand } from '@shared/app-brand/app-brand';
import { catchError, of } from 'rxjs';

type CallbackStatus = 'loading' | 'error';

@Component({
    selector: 'cq-oidc-callback-page',
    imports: [RouterLink, AppBrand],
    templateUrl: './oidc-callback-page.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class OidcCallbackPage {
    private readonly runtimeConfig = inject(RuntimeConfigService);
    private readonly oidcAuth = inject(OidcAuthService);
    private readonly route = inject(ActivatedRoute);
    private readonly router = inject(Router);
    private readonly destroyRef = inject(DestroyRef);

    readonly status = signal<CallbackStatus>('loading');
    readonly message = signal('Completing sign-in...');
    readonly isLoading = computed(() => this.status() === 'loading');

    private readonly returnUrl = this.resolveReturnUrl();

    constructor() {
        this.handleCallback();
    }

    retry(): void {
        if (this.runtimeConfig.authMode !== 'oidc') {
            void this.router.navigate(['/auth', 'login']);
            return;
        }

        this.status.set('loading');
        this.message.set('Completing sign-in...');
        this.oidcAuth.startLogin(this.returnUrl);
    }

    private handleCallback(): void {
        if (this.runtimeConfig.authMode !== 'oidc') {
            void this.router.navigate(['/auth', 'login']);
            return;
        }

        this.oidcAuth
            .refresh()
            .pipe(
                takeUntilDestroyed(this.destroyRef),
                catchError(() => {
                    this.status.set('error');
                    this.message.set('Could not complete sign-in. Please try again.');
                    return of(null);
                }),
            )
            .subscribe((result) => {
                if (!result) {
                    return;
                }
                void this.router.navigateByUrl(this.returnUrl);
            });
    }

    private resolveReturnUrl(): string {
        const candidate = (this.route.snapshot.queryParamMap.get('returnUrl') ?? '').trim();
        if (
            !candidate ||
            candidate === '/' ||
            candidate.startsWith('/login') ||
            candidate.startsWith('/auth')
        ) {
            return '/';
        }
        return candidate;
    }
}
