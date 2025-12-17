import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';

import { CRONIQ_API_CLIENT, type CroniqApiClient } from 'data-access';

import { AuthSessionService } from './auth-session.service';
import { PasswordAuthService } from './password-auth.service';

describe('PasswordAuthService', () => {
    let service: PasswordAuthService;
    let apiClient: Pick<CroniqApiClient, 'passwordLogin'>;
    let authSession: Pick<AuthSessionService, 'storeSessionToken' | 'storeRefreshToken' | 'clearRefreshToken'>;

    beforeEach(() => {
        apiClient = {
            passwordLogin: vi.fn(),
        };

        authSession = {
            storeSessionToken: vi.fn(),
            storeRefreshToken: vi.fn(),
            clearRefreshToken: vi.fn(),
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
        (apiClient.passwordLogin as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
            accessToken: 'access-123',
            expiresIn: 120,
            refreshToken: 'refresh-xyz',
        });

        vi.useFakeTimers();
        vi.setSystemTime(new Date('2025-12-17T00:00:00.000Z'));

        const result = await service.login({ username: 'alice', password: 'secret' });

        expect(apiClient.passwordLogin).toHaveBeenCalledWith({
            username: 'alice',
            password: 'secret',
            tenantReference: null,
            environmentTag: null,
            scopes: null,
            audience: null,
        });

        expect(authSession.storeSessionToken).toHaveBeenCalledWith('access-123', {
            expiresAt: '2025-12-17T00:02:00.000Z',
        });
        expect(authSession.storeRefreshToken).toHaveBeenCalledWith('refresh-xyz');
        expect(result.token).toBe('access-123');
        expect(result.refreshTokenPresent).toBe(true);

        vi.useRealTimers();
    });

    it('accepts plain string responses and clears refresh token', async () => {
        (apiClient.passwordLogin as unknown as ReturnType<typeof vi.fn>).mockResolvedValue('access-token-string');

        const result = await service.login({ username: 'alice', password: 'secret' });

        expect(authSession.storeSessionToken).toHaveBeenCalledWith('access-token-string', { expiresAt: null });
        expect(authSession.clearRefreshToken).toHaveBeenCalled();
        expect(result.refreshTokenPresent).toBe(false);
    });

    it('throws when response does not contain an access token', async () => {
        (apiClient.passwordLogin as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
            expiresIn: 60,
        });

        await expect(service.login({ username: 'alice', password: 'secret' })).rejects.toThrow(
            /unsupported response shape/i,
        );

        expect(authSession.storeSessionToken).not.toHaveBeenCalled();
    });
});
