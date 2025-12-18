import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Field, form, required } from '@angular/forms/signals';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { AuthSessionService } from '@core/auth/auth-session.service';
import { PasswordAuthService } from '@core/auth/password-auth.service';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { AppBrand } from '@shared/app-brand/app-brand';

@Component({
    selector: 'cq-login-page',
    imports: [RouterLink, Field, AppBrand],
    templateUrl: './login-page.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LoginPage {
    private readonly authSession = inject(AuthSessionService);
    private readonly passwordAuth = inject(PasswordAuthService);
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

    readonly loginModel = signal({
        username: '',
        password: '',
    });

    readonly usernameEmpty = computed(() => this.loginModel().username.trim().length === 0);
    readonly passwordEmpty = computed(() => this.loginModel().password.length === 0);
    readonly showValidation = computed(() => this.submitAttempted() && (this.usernameEmpty() || this.passwordEmpty()));

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
        required(fieldPath.username, { message: 'Bitte Username angeben.' });
        required(fieldPath.password, { message: 'Bitte Passwort angeben.' });
    });

    async login(): Promise<void> {
        if (this.busy()) {
            return;
        }

        this.submitAttempted.set(true);

        if (this.loginForm().invalid()) {
            this.lastAction.set('Bitte Username und Passwort angeben.');
            this.lastActionTone.set('error');
            return;
        }

        const username = this.loginModel().username.trim();
        const password = this.loginModel().password;
        if (!username || !password) {
            this.lastAction.set('Bitte Username und Passwort angeben.');
            this.lastActionTone.set('error');
            return;
        }

        this.busy.set(true);
        this.lastAction.set(null);
        this.lastActionTone.set(null);

        try {
            const result = await this.passwordAuth.login({ username, password });
            const tenantId = this.tenantContext.snapshot().tenantId.trim();
            const resolvedTenantId = result.tenantId?.trim();
            const tenantReference = result.tenantReference?.trim();
            if (!tenantId) {
                if (resolvedTenantId) {
                    this.tenantContext.setTenantIdentity(resolvedTenantId);
                } else if (tenantReference) {
                    this.tenantContext.setTenantIdentity(tenantReference);
                }
            }
            this.loginModel.update((model) => ({ ...model, password: '' }));
            this.lastAction.set('Login erfolgreich.');
            this.lastActionTone.set('success');
            await this.router.navigateByUrl(this.resolveReturnUrl());
        } catch (error) {
            this.lastAction.set(error instanceof Error ? error.message : 'Login fehlgeschlagen.');
            this.lastActionTone.set('error');
        } finally {
            this.busy.set(false);
        }
    }

    private resolveReturnUrl(): string {
        const fromQuery = (this.route.snapshot.queryParamMap.get('returnUrl') ?? '').trim();
        const fromHistory = (this.router.getCurrentNavigation()?.previousNavigation?.finalUrl?.toString() ?? '').trim();
        const candidate = fromQuery || fromHistory;

        if (!candidate || candidate === '/' || candidate.startsWith('/login')) {
            return '/';
        }

        return candidate;
    }

    clearToken(): void {
        this.authSession.clearSessionToken();
        this.lastAction.set('Token entfernt.');
        this.lastActionTone.set('info');
    }

    onCredentialEdit(): void {
        if (this.lastActionTone() === 'error') {
            this.lastAction.set(null);
            this.lastActionTone.set(null);
        }
    }
}
