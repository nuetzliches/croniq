import { Injectable, inject } from '@angular/core';
import type { Observable } from 'rxjs';
import { RuntimeConfigService } from '@core/runtime-config.service';
import { OidcAuthService } from './oidc-auth.service';
import { PasswordAuthService } from './password-auth.service';

@Injectable({ providedIn: 'root' })
export class AuthSignOutService {
    private readonly runtimeConfig = inject(RuntimeConfigService);
    private readonly oidcAuth = inject(OidcAuthService);
    private readonly passwordAuth = inject(PasswordAuthService);

    signOut(): Observable<void> {
        return this.runtimeConfig.authMode === 'oidc'
            ? this.oidcAuth.logout()
            : this.passwordAuth.logout();
    }
}
