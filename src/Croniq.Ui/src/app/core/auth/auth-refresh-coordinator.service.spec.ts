import { provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';
import { AuthRefreshCoordinator } from './auth-refresh-coordinator.service';
import { AuthSessionService } from './auth-session.service';
import type { PasswordRefreshResult } from './password-auth.service';
import { PasswordAuthService } from './password-auth.service';

class AuthSessionStub {
    readonly sessionToken = signal<{ value: string; expiresAt?: string | null } | null>(null);
    readonly refreshToken = signal<string | null>(null);

    getSessionToken = vi.fn(() => this.sessionToken()?.value ?? null);
    clearAuthState = vi.fn();
}

class PasswordAuthStub {
    refresh = vi.fn<() => Promise<PasswordRefreshResult>>();
}

class RouterStub {
    navigate = vi.fn<Router['navigate']>().mockResolvedValue(true as never);
}

describe('AuthRefreshCoordinator', () => {
    let coordinator: AuthRefreshCoordinator;
    let authSession: AuthSessionStub;
    let passwordAuth: PasswordAuthStub;
    let router: RouterStub;

    beforeEach(() => {
        TestBed.configureTestingModule({
            providers: [
                provideZonelessChangeDetection(),
                AuthRefreshCoordinator,
                { provide: AuthSessionService, useClass: AuthSessionStub },
                { provide: PasswordAuthService, useClass: PasswordAuthStub },
                { provide: Router, useClass: RouterStub },
            ],
        });

        coordinator = TestBed.inject(AuthRefreshCoordinator);
        authSession = TestBed.inject(AuthSessionService) as unknown as AuthSessionStub;
        passwordAuth = TestBed.inject(PasswordAuthService) as unknown as PasswordAuthStub;
        router = TestBed.inject(Router) as unknown as RouterStub;

        vi.useFakeTimers();
        vi.setSystemTime(new Date('2025-12-19T12:00:00.000Z'));
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('returns current token when no refresh token is present', async () => {
        authSession.sessionToken.set({ value: 'access-1', expiresAt: '2025-12-19T13:00:00.000Z' });
        authSession.refreshToken.set(null);

        const token = await coordinator.ensureFreshAccessToken();

        expect(token).toBe('access-1');
        expect(passwordAuth.refresh).not.toHaveBeenCalled();
    });

    it('does not refresh when expiry is outside the skew window', async () => {
        authSession.sessionToken.set({ value: 'access-1', expiresAt: '2025-12-19T12:10:00.000Z' });
        authSession.refreshToken.set('refresh-1');

        const token = await coordinator.ensureFreshAccessToken();

        expect(token).toBe('access-1');
        expect(passwordAuth.refresh).not.toHaveBeenCalled();
    });

    it('refreshes when token is missing but refresh token exists', async () => {
        authSession.sessionToken.set(null);
        authSession.refreshToken.set('refresh-1');

        passwordAuth.refresh.mockResolvedValue({
            storedInSession: true,
            token: 'access-2',
            expiresAt: null,
            refreshTokenPresent: true,
            passwordChangeRequired: false,
            raw: {},
        });

        const token = await coordinator.ensureFreshAccessToken();

        expect(token).toBe('access-2');
        expect(passwordAuth.refresh).toHaveBeenCalledTimes(1);
    });

    it('refreshes when expiry is within the skew window', async () => {
        authSession.sessionToken.set({ value: 'access-1', expiresAt: '2025-12-19T12:00:30.000Z' });
        authSession.refreshToken.set('refresh-1');

        passwordAuth.refresh.mockResolvedValue({
            storedInSession: true,
            token: 'access-2',
            expiresAt: null,
            refreshTokenPresent: true,
            passwordChangeRequired: false,
            raw: {},
        });

        const token = await coordinator.ensureFreshAccessToken();

        expect(token).toBe('access-2');
        expect(passwordAuth.refresh).toHaveBeenCalledTimes(1);
    });

    it('single-flights concurrent forceRefresh calls', async () => {
        authSession.refreshToken.set('refresh-1');

        let resolveRefresh!: (value: PasswordRefreshResult) => void;
        const refreshPromise = new Promise<PasswordRefreshResult>((resolve) => {
            resolveRefresh = resolve;
        });
        passwordAuth.refresh.mockReturnValue(refreshPromise);

        const p1 = coordinator.forceRefresh();
        const p2 = coordinator.forceRefresh();

        expect(passwordAuth.refresh).toHaveBeenCalledTimes(1);

        resolveRefresh({
            storedInSession: true,
            token: 'access-2',
            expiresAt: null,
            refreshTokenPresent: true,
            passwordChangeRequired: false,
            raw: {},
        });

        const [t1, t2] = await Promise.all([p1, p2]);

        expect(t1).toBe('access-2');
        expect(t2).toBe('access-2');
    });

    it('navigates to /change-password when refresh indicates passwordChangeRequired', async () => {
        authSession.refreshToken.set('refresh-1');

        passwordAuth.refresh.mockResolvedValue({
            storedInSession: true,
            token: 'access-2',
            expiresAt: null,
            refreshTokenPresent: true,
            passwordChangeRequired: true,
            raw: {},
        });

        const token = await coordinator.forceRefresh();

        expect(token).toBe('access-2');
        expect(router.navigate).toHaveBeenCalledWith(['/change-password']);
    });

    it('clears auth state and navigates to /login when refresh fails', async () => {
        authSession.refreshToken.set('refresh-1');
        passwordAuth.refresh.mockRejectedValue(new Error('nope'));

        const token = await coordinator.forceRefresh();

        expect(token).toBe(null);
        expect(authSession.clearAuthState).toHaveBeenCalled();
        expect(router.navigate).toHaveBeenCalledWith(['/login']);
    });
});
