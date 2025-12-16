import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';

import { type CroniqApiClient, CRONIQ_API_CLIENT } from 'data-access';

import { tryIsoFromUnknown } from '../time/clock';
import { AuthSessionService } from './auth-session.service';
import { TenantTokenEndpointService } from './token-endpoint.service';

describe('TenantTokenEndpointService', () => {
    let service: TenantTokenEndpointService;
    let apiClient: Pick<CroniqApiClient, 'issueTenantToken'>;
    let authSession: Pick<AuthSessionService, 'storeSessionToken'>;

    beforeEach(() => {
        apiClient = {
            issueTenantToken: vi.fn(),
        };
        authSession = {
            storeSessionToken: vi.fn(),
        };

        TestBed.configureTestingModule({
            providers: [
                provideZonelessChangeDetection(),
                TenantTokenEndpointService,
                { provide: CRONIQ_API_CLIENT, useValue: apiClient },
                { provide: AuthSessionService, useValue: authSession },
            ],
        });

        service = TestBed.inject(TenantTokenEndpointService);
    });

    it('normalizes expiresAt from epoch ms and persists when requested', async () => {
        const expiresAtMs = 1730000000000;
        (apiClient.issueTenantToken as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
            token: 'tenant-token-123',
            expiresAt: expiresAtMs,
        });

        const result = await service.issueTenantToken({
            tenantId: 'cron-lab',
            clientId: 'ui',
            persistInSession: true,
        });

        const expectedExpiry = tryIsoFromUnknown(expiresAtMs);
        expect(result.token).toBe('tenant-token-123');
        expect(result.expiresAt).toBe(expectedExpiry);
        expect(result.storedInSession).toBe(true);
        expect(authSession.storeSessionToken).toHaveBeenCalledWith('tenant-token-123', { expiresAt: expectedExpiry });
    });

    it('falls back to fallbackExpiry when response has no expiresAt', async () => {
        (apiClient.issueTenantToken as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
            token: 'tenant-token-abc',
        });

        const result = await service.issueTenantToken({
            tenantId: 'cron-lab',
            clientId: 'ui',
            persistInSession: true,
            fallbackExpiry: '2025-12-16T00:00:00.000Z',
        });

        expect(result.expiresAt).toBe('2025-12-16T00:00:00.000Z');
        expect(authSession.storeSessionToken).toHaveBeenCalledWith('tenant-token-abc', {
            expiresAt: '2025-12-16T00:00:00.000Z',
        });
    });

    it('returns null token and does not persist when response shape is invalid', async () => {
        (apiClient.issueTenantToken as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
            expiresAt: '2025-12-16T00:00:00.000Z',
        });

        const result = await service.issueTenantToken({
            tenantId: 'cron-lab',
            clientId: 'ui',
            persistInSession: true,
        });

        expect(result.token).toBeNull();
        expect(result.storedInSession).toBe(false);
        expect(authSession.storeSessionToken).not.toHaveBeenCalled();
    });
});
