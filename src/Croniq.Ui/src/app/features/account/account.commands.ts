import { Provider, inject } from '@angular/core';
import { Router } from '@angular/router';
import { AuthSignOutService } from '@core/auth/auth-sign-out.service';
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
        const authSignOut = inject(AuthSignOutService);
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
                    authSignOut
                        .signOut()
                        .pipe(finalize(() => void router.navigate(['/auth', 'login'])))
                        .subscribe();
                },
            },
        ];
    },
};
