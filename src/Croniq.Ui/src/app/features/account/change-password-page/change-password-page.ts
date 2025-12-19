import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Field, form, required } from '@angular/forms/signals';
import { Router, RouterLink } from '@angular/router';
import { PasswordAuthService } from '@core/auth/password-auth.service';

@Component({
    selector: 'cq-change-password-page',
    imports: [Field, RouterLink],
    templateUrl: './change-password-page.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ChangePasswordPage {
    private readonly auth = inject(PasswordAuthService);
    private readonly router = inject(Router);

    readonly busy = signal(false);
    readonly submitAttempted = signal(false);
    readonly lastAction = signal<string | null>(null);
    readonly lastActionTone = signal<'info' | 'error' | 'success' | null>(null);

    readonly model = signal({
        currentPassword: '',
        newPassword: '',
    });

    readonly currentPasswordEmpty = computed(() => this.model().currentPassword.length === 0);
    readonly newPasswordEmpty = computed(() => this.model().newPassword.length === 0);

    readonly showValidation = computed(
        () => this.submitAttempted() && (this.currentPasswordEmpty() || this.newPasswordEmpty()),
    );

    readonly changePasswordForm = form(this.model, (fieldPath) => {
        required(fieldPath.currentPassword, { message: 'Bitte aktuelles Passwort angeben.' });
        required(fieldPath.newPassword, { message: 'Bitte neues Passwort angeben.' });
    });

    async submit(): Promise<void> {
        if (this.busy()) {
            return;
        }

        this.submitAttempted.set(true);

        if (this.changePasswordForm().invalid()) {
            this.lastAction.set('Bitte aktuelles und neues Passwort angeben.');
            this.lastActionTone.set('error');
            return;
        }

        const currentPassword = this.model().currentPassword;
        const newPassword = this.model().newPassword;

        this.busy.set(true);
        this.lastAction.set(null);
        this.lastActionTone.set(null);

        try {
            await this.auth.changePassword({ currentPassword, newPassword });
            this.lastAction.set('Passwort geändert. Bitte neu einloggen.');
            this.lastActionTone.set('success');
            await this.router.navigateByUrl('/login');
        } catch (error) {
            this.lastAction.set(error instanceof Error ? error.message : 'Passwort ändern fehlgeschlagen.');
            this.lastActionTone.set('error');
        } finally {
            this.busy.set(false);
        }
    }

    onEdit(): void {
        if (this.lastActionTone() === 'error') {
            this.lastAction.set(null);
            this.lastActionTone.set(null);
        }
    }
}
