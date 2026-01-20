import { provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { ActivatedRoute, Router, UrlTree, convertToParamMap, provideRouter } from '@angular/router';
import { AuthRefreshCoordinator } from '@core/auth/auth-refresh-coordinator.service';
import { AuthSessionService } from '@core/auth/auth-session.service';
import { OidcAuthService } from '@core/auth/oidc-auth.service';
import { PasswordAuthService } from '@core/auth/password-auth.service';
import { redirectIfSessionTokenGuard } from '@core/auth/redirect-if-session-token.guard';
import { RuntimeConfigService } from '@core/runtime-config.service';
import { of } from 'rxjs';
import { LoginPage } from './login-page';

class AuthSessionStub {
    readonly sessionToken = signal<{ value: string } | null>(null);
    readonly sessionTokenExpired = signal(false);
    readonly refreshToken = signal<string | null>(null);
    readonly tenantId = signal<string | null>(null);
    readonly passwordChangeRequired = signal(false);

    getSessionToken(): string | null {
        return this.sessionToken()?.value ?? null;
    }

    storeSessionToken = vi.fn();
    clearSessionToken = vi.fn();
    clearAuthState = vi.fn();
    storeRefreshToken = vi.fn();
    clearRefreshToken = vi.fn();
}

class PasswordAuthStub {
    login = vi.fn();
}

class OidcAuthStub {
    startLogin = vi.fn();
}

class AuthRefreshCoordinatorStub {
    ensureFreshAccessToken = vi.fn(() => of(null));
}

class RuntimeConfigStub {
    defaultTenantId = '';
    authMode: 'password' | 'oidc' = 'password';
}

describe('LoginPage', () => {
    let component: LoginPage;
    let fixture: ComponentFixture<LoginPage>;
    let router: Router;
    let authSession: AuthSessionStub;
    let passwordAuth: PasswordAuthStub;
    let runtimeConfig: RuntimeConfigStub;

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
                { provide: AuthRefreshCoordinator, useClass: AuthRefreshCoordinatorStub },
                { provide: OidcAuthService, useClass: OidcAuthStub },
                { provide: PasswordAuthService, useClass: PasswordAuthStub },
                { provide: RuntimeConfigService, useClass: RuntimeConfigStub },
            ],
        }).compileComponents();

        router = TestBed.inject(Router);
        authSession = TestBed.inject(AuthSessionService) as unknown as AuthSessionStub;
        passwordAuth = TestBed.inject(PasswordAuthService) as unknown as PasswordAuthStub;
        runtimeConfig = TestBed.inject(RuntimeConfigService) as unknown as RuntimeConfigStub;

        fixture = TestBed.createComponent(LoginPage);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });

    it('redirects away from /auth/login when already authenticated', async () => {
        authSession.sessionToken.set({ value: 'access-token' });

        const result = TestBed.runInInjectionContext(() =>
            redirectIfSessionTokenGuard({
                queryParamMap: convertToParamMap({ returnUrl: '/jobs' }),
            } as never,
                { url: '/auth/login' } as never),
        );

        expect(result).toBeInstanceOf(UrlTree);
        expect(router.serializeUrl(result as UrlTree)).toBe('/jobs');
    });

    it('redirects to returnUrl after successful login', async () => {
        const navigateSpy = vi.spyOn(router, 'navigateByUrl').mockResolvedValue(true as never);
        passwordAuth.login.mockReturnValue(of({
            storedInSession: true,
            token: 'access-token',
            expiresAt: null,
            refreshTokenPresent: false,
            passwordChangeRequired: false,
            tenantId: 'default',
            raw: {},
        }));

        component.loginModel.set({ tenantId: 'tenant-a', username: 'admin', password: 'admin' });
        await component.onSubmit(new SubmitEvent('submit'));

        expect(navigateSpy).toHaveBeenCalledWith('/jobs');
    });

    it('forces /change-password when password change is required', async () => {
        const navigateSpy = vi.spyOn(router, 'navigateByUrl').mockResolvedValue(true as never);
        passwordAuth.login.mockReturnValue(of({
            storedInSession: true,
            token: 'access-token',
            expiresAt: null,
            refreshTokenPresent: false,
            passwordChangeRequired: true,
            tenantId: 'default',
            raw: {},
        }));

        component.loginModel.set({ tenantId: 'tenant-a', username: 'admin', password: 'admin' });
        await component.onSubmit(new SubmitEvent('submit'));

        expect(navigateSpy).toHaveBeenCalledWith('/auth/change-password');
    });

    it('prefills tenantId from runtime config', async () => {
        runtimeConfig.defaultTenantId = 'tenant-config';

        const localFixture = TestBed.createComponent(LoginPage);
        const localComponent = localFixture.componentInstance;
        await localFixture.whenStable();

        expect(localComponent.loginModel().tenantId).toBe('tenant-config');
    });
});
