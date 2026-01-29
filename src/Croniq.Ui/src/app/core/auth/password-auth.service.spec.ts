import { provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { CRONIQ_API_CLIENT, type CroniqApiClient } from 'data-access';
import { firstValueFrom, of } from 'rxjs';
import { AuthSessionService } from './auth-session.service';
import { PasswordAuthService } from './password-auth.service';

describe('PasswordAuthService', () => {
    let service: PasswordAuthService;
    let apiClient: Pick<CroniqApiClient, 'passwordLogin'>;
    let authSession: Pick<
        AuthSessionService,
        'storeSessionToken' | 'storeRefreshToken' | 'clearRefreshToken' | 'storeTenantId' | 'refreshToken'
    >;

    beforeEach(() => {
        apiClient = {
            passwordLogin: vi.fn(),
        };

        const refreshTokenSignal = signal<string | null>(null);
        authSession = {
            storeSessionToken: vi.fn(),
            storeRefreshToken: vi.fn(),
            clearRefreshToken: vi.fn(),
            storeTenantId: vi.fn(),
            refreshToken: refreshTokenSignal,
        };

        TestBed.configureTestingModule({
            providers: [
                provideZonelessChangeDetection(),
                PasswordAuthService,
                { provide: CRONIQ_API_CLIENT, useValue: apiClient },
                { provide: AuthSessionService, useValue: authSession },
            ],
        });

        service = TestBed.inject(PasswordAuthService);
    });

    it('sends tenant/environment as null and stores access token', async () => {
        (apiClient.passwordLogin as unknown as ReturnType<typeof vi.fn>).mockReturnValue(of({
            accessToken: 'access-123',
            expiresIn: 120,
            refreshToken: 'refresh-xyz',
            tenantId: 'default',
        }));

        vi.useFakeTimers();
        vi.setSystemTime(new Date('2025-12-17T00:00:00.000Z'));

        const result = await firstValueFrom(service.login({ username: 'alice', password: 'secret', tenantId: 'default' }));

        expect(apiClient.passwordLogin).toHaveBeenCalledWith({
            username: 'alice',
            password: 'secret',
            tenantId: 'default',
            environmentTag: null,
            scopes: null,
            audience: null,
        });

        expect(authSession.storeSessionToken).toHaveBeenCalledWith('access-123', {
            expiresAt: '2025-12-17T00:02:00.000Z',
            passwordChangeRequired: false,
        });
        expect(authSession.storeRefreshToken).toHaveBeenCalledWith('refresh-xyz');
        expect(result.token).toBe('access-123');
        expect(result.refreshTokenPresent).toBe(true);
        expect(result.passwordChangeRequired).toBe(false);
        expect(result.tenantId).toBe('default');

        vi.useRealTimers();
    });

    it('accepts plain string responses and clears refresh token', async () => {
        // Mock response must include tenantId claim or property if the service requires it.
        // Since the service throws "missing tenantId in response" if not present,
        // and "plain string" response usually implies just the token, we might need to mock the token decoding
        // OR the service logic handles plain string by decoding it.
        // Looking at the service code (not fully visible but implied), it calls `this.extract(response)`.
        // If response is string, it treats it as token. Then it tries to get tenantId from it.
        // So we need a token that has a tenant claim.

        const tokenWithTenant =
            'eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.' +
            'eyJ0ZW5hbnQiOiJkZWZhdWx0In0.' + // tenant: default
            'signature';

        (apiClient.passwordLogin as unknown as ReturnType<typeof vi.fn>).mockReturnValue(of(tokenWithTenant));

        const result = await firstValueFrom(service.login({ username: 'alice', password: 'secret', tenantId: 'default' }));

        expect(authSession.storeSessionToken).toHaveBeenCalledWith(tokenWithTenant, {
            expiresAt: null,
            passwordChangeRequired: false,
        });
        expect(authSession.storeRefreshToken).not.toHaveBeenCalled();
        expect(result.refreshTokenPresent).toBe(false);
        expect(result.passwordChangeRequired).toBe(false);
        expect(result.tenantId).toBe('default');
    });

    it('throws when response does not contain an access token', async () => {
        (apiClient.passwordLogin as unknown as ReturnType<typeof vi.fn>).mockReturnValue(of({
            expiresIn: 60,
        }));

        await expect(firstValueFrom(service.login({ username: 'alice', password: 'secret', tenantId: 'default' }))).rejects.toThrow(
            /unsupported response shape/i,
        );

        expect(authSession.storeSessionToken).not.toHaveBeenCalled();
    });

    it('extracts tenantId from jwt tenant claim', async () => {
        // header: {"alg":"none","typ":"JWT"}
        // payload: {"tenant":"tn_test"}
        const token =
            'eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.' +
            'eyJ0ZW5hbnQiOiJ0bl90ZXN0In0.' +
            'signature';

        (apiClient.passwordLogin as unknown as ReturnType<typeof vi.fn>).mockReturnValue(of({
            accessToken: token,
            expiresIn: 120,
        }));

        const result = await firstValueFrom(service.login({ username: 'alice', password: 'secret', tenantId: 'default' }));

        expect(result.tenantId).toBe('tn_test');
    });
});
