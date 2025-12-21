import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Field, form, required } from '@angular/forms/signals';
import { Router, RouterLink } from '@angular/router';
import { PasswordAuthService } from '@core/auth/password-auth.service';
import { finalize } from 'rxjs';

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
        required(fieldPath.currentPassword, { message: 'Current password is required.' });
        required(fieldPath.newPassword, { message: 'New password is required.' });
    });

    submit(): void {
        if (this.busy()) {
            return;
        }

        this.submitAttempted.set(true);

        if (this.changePasswordForm().invalid()) {
            this.lastAction.set('Please enter your current password and a new password.');
            this.lastActionTone.set('error');
            return;
        }

        const currentPassword = this.model().currentPassword;
        const newPassword = this.model().newPassword;

        this.busy.set(true);
        this.lastAction.set(null);
        this.lastActionTone.set(null);

        this.auth
            .changePassword({ currentPassword, newPassword })
            .pipe(finalize(() => this.busy.set(false)))
            .subscribe({
                next: () => {
                    this.lastAction.set('Password changed. Please sign in again.');
                    this.lastActionTone.set('success');
                    void this.router.navigateByUrl('/auth/login');
                },
                error: (error: unknown) => {
                    this.lastAction.set(error instanceof Error ? error.message : 'Change password failed.');
                    this.lastActionTone.set('error');
                },
            });
    }

    onEdit(): void {
        if (this.lastActionTone() === 'error') {
            this.lastAction.set(null);
            this.lastActionTone.set(null);
        }
    }
}
