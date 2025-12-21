import { Provider, inject } from '@angular/core';
import { Router } from '@angular/router';
import { PasswordAuthService } from '@core/auth/password-auth.service';
import { COMMAND_PALETTE_COMMANDS, type CommandPaletteCommand } from '@shared/command-palette/command-palette.controller';
import { finalize } from 'rxjs';

const ACCOUNT_COMMANDS: ReadonlyArray<CommandPaletteCommand> = [
    {
        id: 'account-change-password',
        label: 'Account · Change password',
        path: 'account/change-password',
        description: 'Replace your current password with a new one',
        keywords: ['password', 'security', 'account'],
    },
];

export const ACCOUNT_COMMANDS_PROVIDER: Provider = {
    provide: COMMAND_PALETTE_COMMANDS,
    multi: true,
    useFactory: (): ReadonlyArray<CommandPaletteCommand> => {
        const passwordAuth = inject(PasswordAuthService);
        const router = inject(Router);

        return [
            ...ACCOUNT_COMMANDS,
            {
                id: 'account-logout',
                label: 'Account · Logout',
                path: 'login',
                description: 'Sign out and return to login',
                keywords: ['logout', 'sign out', 'session'],
                execute: () => {
                    passwordAuth
                        .logout()
                        .pipe(finalize(() => void router.navigate(['/auth', 'login'])))
                        .subscribe();
                },
            },
        ];
    },
};
