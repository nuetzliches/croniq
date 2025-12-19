import { provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { ActivatedRoute, Router, UrlTree, convertToParamMap, provideRouter } from '@angular/router';
import { AuthSessionService } from '@core/auth/auth-session.service';
import { PasswordAuthService } from '@core/auth/password-auth.service';
import { redirectIfSessionTokenGuard } from '@core/auth/redirect-if-session-token.guard';
import { LoginPage } from './login-page';

class AuthSessionStub {
    readonly sessionToken = signal<{ value: string } | null>(null);
    readonly sessionTokenExpired = signal(false);
    readonly refreshToken = signal<string | null>(null);

    getSessionToken(): string | null {
        return this.sessionToken()?.value ?? null;
    }

    storeSessionToken = vi.fn();
    clearSessionToken = vi.fn();
    storeRefreshToken = vi.fn();
    clearRefreshToken = vi.fn();
}

class PasswordAuthStub {
    login = vi.fn();
}

describe('LoginPage', () => {
    let component: LoginPage;
    let fixture: ComponentFixture<LoginPage>;
    let router: Router;
    let authSession: AuthSessionStub;
    let passwordAuth: PasswordAuthStub;

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [LoginPage],
            providers: [
                provideZonelessChangeDetection(),
                provideRouter([]),
                {
                    provide: ActivatedRoute,
                    useValue: {
                        snapshot: {
                            queryParamMap: convertToParamMap({ returnUrl: '/jobs' }),
                        },
                    },
                },
                { provide: AuthSessionService, useClass: AuthSessionStub },
                { provide: PasswordAuthService, useClass: PasswordAuthStub },
            ],
        }).compileComponents();

        router = TestBed.inject(Router);
        authSession = TestBed.inject(AuthSessionService) as unknown as AuthSessionStub;
        passwordAuth = TestBed.inject(PasswordAuthService) as unknown as PasswordAuthStub;

        fixture = TestBed.createComponent(LoginPage);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });

    it('redirects away from /login when already authenticated', async () => {
        authSession.sessionToken.set({ value: 'access-token' });

        const result = TestBed.runInInjectionContext(() =>
            redirectIfSessionTokenGuard({
                queryParamMap: convertToParamMap({ returnUrl: '/jobs' }),
            } as never,
                { url: '/login' } as never),
        );

        expect(result).toBeInstanceOf(UrlTree);
        expect(router.serializeUrl(result as UrlTree)).toBe('/jobs');
    });

    it('redirects to returnUrl after successful login', async () => {
        const navigateSpy = vi.spyOn(router, 'navigateByUrl').mockResolvedValue(true as never);
        passwordAuth.login.mockResolvedValue({
            storedInSession: true,
            token: 'access-token',
            expiresAt: null,
            refreshTokenPresent: false,
            passwordChangeRequired: false,
            tenantId: null,
            tenantReference: null,
            raw: {},
        });

        component.loginModel.set({ username: 'admin', password: 'admin' });
        await component.login();

        expect(navigateSpy).toHaveBeenCalledWith('/jobs');
    });

    it('forces /change-password when password change is required', async () => {
        const navigateSpy = vi.spyOn(router, 'navigateByUrl').mockResolvedValue(true as never);
        passwordAuth.login.mockResolvedValue({
            storedInSession: true,
            token: 'access-token',
            expiresAt: null,
            refreshTokenPresent: false,
            passwordChangeRequired: true,
            tenantId: null,
            tenantReference: null,
            raw: {},
        });

        component.loginModel.set({ username: 'admin', password: 'admin' });
        await component.login();

        expect(navigateSpy).toHaveBeenCalledWith('/change-password');
    });
});
