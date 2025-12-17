import { CommonModule } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Router, RouterLink } from '@angular/router';
import { AuthSessionService } from '@core/auth/auth-session.service';
import { PasswordAuthService } from '@core/auth/password-auth.service';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';

@Component({
    selector: 'cq-login-page',
    imports: [CommonModule, RouterLink],
    templateUrl: './login-page.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LoginPage {
    private readonly authSession = inject(AuthSessionService);
    private readonly passwordAuth = inject(PasswordAuthService);
    private readonly tenantContext = inject(TenantContextService);
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
    readonly busy = signal(false);

    async login(usernameInput: HTMLInputElement, passwordInput: HTMLInputElement): Promise<void> {
        if (this.busy()) {
            return;
        }

        const username = usernameInput.value.trim();
        const password = passwordInput.value;
        if (!username || !password) {
            this.lastAction.set('Bitte Username und Passwort angeben.');
            return;
        }

        this.busy.set(true);
        this.lastAction.set(null);

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
            passwordInput.value = '';
            this.lastAction.set('Login erfolgreich.');
            await this.router.navigateByUrl('/schedules');
        } catch (error) {
            this.lastAction.set(error instanceof Error ? error.message : 'Login fehlgeschlagen.');
        } finally {
            this.busy.set(false);
        }
    }

    clearToken(): void {
        this.authSession.clearSessionToken();
        this.lastAction.set('Token entfernt.');
    }
}
