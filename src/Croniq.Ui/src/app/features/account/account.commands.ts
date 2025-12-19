import { Provider, inject } from '@angular/core';
import { Router } from '@angular/router';
import { PasswordAuthService } from '@core/auth/password-auth.service';
import { COMMAND_PALETTE_COMMANDS, type CommandPaletteCommand } from '@shared/command-palette/command-palette.controller';

const ACCOUNT_COMMANDS: ReadonlyArray<CommandPaletteCommand> = [
    {
        id: 'account-change-password',
        label: 'Account · Passwort ändern',
        path: 'change-password',
        description: 'Aktuelles Passwort durch ein neues ersetzen',
        keywords: ['password', 'passwort', 'security', 'konto'],
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
                description: 'Abmelden und zurück zum Login',
                keywords: ['logout', 'abmelden', 'sign out', 'session'],
                execute: async () => {
                    await passwordAuth.logout();
                    await router.navigate(['/login']);
                },
            },
        ];
    },
};
