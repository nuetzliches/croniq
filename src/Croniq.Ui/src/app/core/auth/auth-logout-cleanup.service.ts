import { Injectable, inject } from '@angular/core';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { UiPreferencesService } from '@core/preferences/ui-preferences.service';
import { AuthSessionService } from './auth-session.service';

@Injectable({ providedIn: 'root' })
export class AuthLogoutCleanupService {
    private readonly authSession = inject(AuthSessionService);
    private readonly tenantContext = inject(TenantContextService);
    private readonly uiPreferences = inject(UiPreferencesService);

    clearAll(): void {
        const tenantId = this.authSession.tenantId()?.trim() ?? '';
        this.authSession.clearAuthState();
        this.tenantContext.resetContext();

        if (tenantId) {
            void this.uiPreferences.clearForTenant(tenantId);
        } else {
            this.uiPreferences.resetToDefaults();
        }
    }
}