import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Field, form, required, submit } from '@angular/forms/signals';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { AuthSessionService } from '@core/auth/auth-session.service';
import { PasswordAuthService } from '@core/auth/password-auth.service';
import { RuntimeConfigService } from '@core/runtime-config.service';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { AppBrand } from '@shared/app-brand/app-brand';
import { finalize } from 'rxjs';

@Component({
    selector: 'cq-login-page',
    imports: [RouterLink, Field, AppBrand],
    templateUrl: './login-page.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LoginPage {
    private readonly authSession = inject(AuthSessionService);
    private readonly passwordAuth = inject(PasswordAuthService);
    private readonly runtimeConfig = inject(RuntimeConfigService);
    private readonly tenantContext = inject(TenantContextService);
    private readonly route = inject(ActivatedRoute);
    private readonly router = inject(Router);

    readonly sessionToken = this.authSession.sessionToken;
    readonly sessionTokenExpired = this.authSession.sessionTokenExpired;

    readonly maskedSessionToken = computed(() => {
        const raw = this.sessionToken()?.value?.trim();
        if (!raw) {
            return '—';
        }
        if (raw.length <= 4) {
            return raw;
        }
        return `•••• ${raw.slice(-4)}`;
    });

    readonly lastAction = signal<string | null>(null);
    readonly lastActionTone = signal<'info' | 'error' | 'success' | null>(null);
    readonly busy = signal(false);
    readonly submitAttempted = signal(false);

    readonly defaultTenantId = signal(this.runtimeConfig.defaultTenantId);
    readonly showTenantIdInput = computed(() => this.defaultTenantId().trim().length === 0);
    readonly usernameAutofocus = computed(() => !this.showTenantIdInput());

    readonly loginModel = signal({
        tenantId: this.defaultTenantId().trim() || this.tenantContext.tenantId().trim(),
        username: '',
        password: '',
    });

    readonly tenantIdEmpty = computed(() => this.loginModel().tenantId.trim().length === 0);
    readonly usernameEmpty = computed(() => this.loginModel().username.trim().length === 0);
    readonly passwordEmpty = computed(() => this.loginModel().password.length === 0);
    readonly showValidation = computed(
        () => this.submitAttempted() && (this.tenantIdEmpty() || this.usernameEmpty() || this.passwordEmpty()),
    );

    readonly tenantIdDescribedBy = computed(() => {
        const ids = ['login-tenant-id-hint'];
        if (this.showValidation() && this.tenantIdEmpty()) {
            ids.push('login-tenant-id-error');
        }
        return ids.join(' ');
    });

    readonly usernameDescribedBy = computed(() => {
        const ids = ['login-username-hint'];
        if (this.showValidation() && this.usernameEmpty()) {
            ids.push('login-username-error');
        }
        return ids.join(' ');
    });

    readonly passwordDescribedBy = computed(() => {
        const ids = ['login-password-hint'];
        if (this.showValidation() && this.passwordEmpty()) {
            ids.push('login-password-error');
        }
        return ids.join(' ');
    });

    readonly loginForm = form(this.loginModel, (fieldPath) => {
        required(fieldPath.tenantId, { message: 'Tenant ID is required.' });
        required(fieldPath.username, { message: 'Username is required.' });
        required(fieldPath.password, { message: 'Password is required.' });
    });

    async onSubmit(event: SubmitEvent) {
        event.preventDefault();

        if (this.busy()) {
            return;
        }

        this.submitAttempted.set(true);

        await submit(this.loginForm, async () => {
            const tenantId = this.loginModel().tenantId.trim();
            const username = this.loginModel().username.trim();
            const password = this.loginModel().password;

            this.busy.set(true);
            this.lastAction.set(null);
            this.lastActionTone.set(null);

            this.passwordAuth
                .login({ username, password, tenantId })
                .pipe(finalize(() => this.busy.set(false)))
                .subscribe({
                    next: (result) => {
                        const resolvedTenantId = result.tenantId?.trim();
                        if (resolvedTenantId) {
                            this.tenantContext.setTenantIdentity(resolvedTenantId);
                        } else if (tenantId) {
                            this.tenantContext.setTenantIdentity(tenantId);
                        }
                        this.loginModel.update((model) => ({ ...model, password: '' }));
                        this.lastAction.set('Signed in successfully.');
                        this.lastActionTone.set('success');

                        if (result.passwordChangeRequired) {
                            void this.router.navigateByUrl('/auth/change-password');
                        } else {
                            void this.router.navigateByUrl(this.resolveReturnUrl());
                        }
                    },
                    error: (error: unknown) => {
                        this.lastAction.set(error instanceof Error ? error.message : 'Sign-in failed.');
                        this.lastActionTone.set('error');
                    },
                });
        });
    }

    private resolveReturnUrl(): string {
        const fromQuery = (this.route.snapshot.queryParamMap.get('returnUrl') ?? '').trim();
        const fromHistory = (this.router.currentNavigation()?.previousNavigation?.finalUrl?.toString() ?? '').trim();
        const candidate = fromQuery || fromHistory;

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

    clearToken(): void {
        this.authSession.clearAuthState();
        this.lastAction.set('Token cleared.');
        this.lastActionTone.set('info');
    }

    onCredentialEdit(): void {
        if (this.lastActionTone() === 'error') {
            this.lastAction.set(null);
            this.lastActionTone.set(null);
        }
    }
}
