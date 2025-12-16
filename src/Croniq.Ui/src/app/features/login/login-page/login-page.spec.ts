import { provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';

import { AuthSessionService } from '../../../core/auth/auth-session.service';
import { PasswordAuthService } from '../../../core/auth/password-auth.service';
import { LoginPage } from './login-page';

class AuthSessionStub {
    readonly sessionToken = signal<{ value: string } | null>(null);
    readonly sessionTokenExpired = signal(false);
    readonly refreshToken = signal<string | null>(null);

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

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [LoginPage],
            providers: [
                provideZonelessChangeDetection(),
                provideRouter([]),
                { provide: AuthSessionService, useClass: AuthSessionStub },
                { provide: PasswordAuthService, useClass: PasswordAuthStub },
            ],
        }).compileComponents();

        fixture = TestBed.createComponent(LoginPage);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });
});
