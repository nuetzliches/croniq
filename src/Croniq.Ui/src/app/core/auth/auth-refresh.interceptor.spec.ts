import { HttpErrorResponse, HttpHeaders, HttpRequest, HttpResponse, provideHttpClient, withInterceptors } from '@angular/common/http';
import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';
import { CRONIQ_API_BASE_URL } from 'data-access';
import { defer, firstValueFrom, of, throwError } from 'rxjs';
import { AuthRefreshCoordinator } from './auth-refresh-coordinator.service';
import { authRefreshInterceptor } from './auth-refresh.interceptor';

class RouterStub {
    url = '/jobs';
    navigate = vi.fn<Router['navigate']>().mockResolvedValue(true as never);
}

describe('authRefreshInterceptor', () => {
    let coordinator: { ensureFreshAccessToken: ReturnType<typeof vi.fn>; forceRefresh: ReturnType<typeof vi.fn> };
    let router: RouterStub;

    beforeEach(() => {
        coordinator = {
            ensureFreshAccessToken: vi.fn(),
            forceRefresh: vi.fn(),
        };

        TestBed.configureTestingModule({
            providers: [
                provideZonelessChangeDetection(),
                provideHttpClient(withInterceptors([authRefreshInterceptor])),
                { provide: CRONIQ_API_BASE_URL, useValue: 'https://api.example' },
                { provide: AuthRefreshCoordinator, useValue: coordinator },
                { provide: Router, useClass: RouterStub },
            ],
        });

        router = TestBed.inject(Router) as unknown as RouterStub;
    });

    it('passes through requests that are not API calls', async () => {
        coordinator.ensureFreshAccessToken.mockResolvedValue('access-1');

        const req = new HttpRequest('GET', 'https://other.example/jobs');
        const next = vi.fn((r: HttpRequest<unknown>) => of(new HttpResponse({ status: 200, url: r.url })));

        await firstValueFrom(TestBed.runInInjectionContext(() => authRefreshInterceptor(req, next)));

        expect(next).toHaveBeenCalledTimes(1);
        const forwarded = next.mock.calls[0]![0];
        expect(forwarded.url).toBe('https://other.example/jobs');
        expect(coordinator.ensureFreshAccessToken).not.toHaveBeenCalled();
    });

    it('skips /auth/* endpoints', async () => {
        coordinator.ensureFreshAccessToken.mockResolvedValue('access-1');

        const req = new HttpRequest('POST', 'https://api.example/auth/login', null);
        const next = vi.fn((r: HttpRequest<unknown>) => of(new HttpResponse({ status: 200, url: r.url })));

        await firstValueFrom(TestBed.runInInjectionContext(() => authRefreshInterceptor(req, next)));

        expect(next).toHaveBeenCalledTimes(1);
        expect(coordinator.ensureFreshAccessToken).not.toHaveBeenCalled();
    });

    it('injects Authorization header when a token is available', async () => {
        coordinator.ensureFreshAccessToken.mockResolvedValue('access-1');

        const req = new HttpRequest('GET', 'https://api.example/jobs');
        const next = vi.fn((r: HttpRequest<unknown>) => of(new HttpResponse({ status: 200, url: r.url })));

        await firstValueFrom(TestBed.runInInjectionContext(() => authRefreshInterceptor(req, next)));

        expect(coordinator.ensureFreshAccessToken).toHaveBeenCalledTimes(1);
        expect(next).toHaveBeenCalledTimes(1);
        const forwarded = next.mock.calls[0]![0];
        expect(forwarded.headers.get('Authorization')).toBe('Bearer access-1');
    });

    it('overwrites existing Authorization header when a token is available', async () => {
        coordinator.ensureFreshAccessToken.mockResolvedValue('access-1');

        const req = new HttpRequest('GET', 'https://api.example/jobs', {
            headers: new HttpHeaders({ Authorization: 'Bearer executor-token' }),
        });
        const next = vi.fn((r: HttpRequest<unknown>) => of(new HttpResponse({ status: 200, url: r.url })));

        await firstValueFrom(TestBed.runInInjectionContext(() => authRefreshInterceptor(req, next)));

        const forwarded = next.mock.calls[0]![0];
        expect(forwarded.headers.get('Authorization')).toBe('Bearer access-1');
    });

    it('preserves existing Authorization header when no token is available', async () => {
        coordinator.ensureFreshAccessToken.mockResolvedValue(null);

        const req = new HttpRequest('GET', 'https://api.example/jobs', {
            headers: new HttpHeaders({ Authorization: 'Bearer executor-token' }),
        });
        const next = vi.fn((r: HttpRequest<unknown>) => of(new HttpResponse({ status: 200, url: r.url })));

        await firstValueFrom(TestBed.runInInjectionContext(() => authRefreshInterceptor(req, next)));

        const forwarded = next.mock.calls[0]![0];
        expect(forwarded.headers.get('Authorization')).toBe('Bearer executor-token');
    });

    it('retries once on 401 after a forced refresh', async () => {
        coordinator.ensureFreshAccessToken.mockResolvedValue('access-1');
        coordinator.forceRefresh.mockResolvedValue('access-2');

        const req = new HttpRequest('GET', 'https://api.example/jobs');

        const next = vi.fn((r: HttpRequest<unknown>) =>
            defer(() => {
                if (next.mock.calls.length === 1) {
                    return throwError(
                        () => new HttpErrorResponse({ status: 401, url: r.url, statusText: 'Unauthorized' }),
                    );
                }
                return of(new HttpResponse({ status: 200 }));
            }),
        );

        await firstValueFrom(TestBed.runInInjectionContext(() => authRefreshInterceptor(req, next)));

        expect(coordinator.forceRefresh).toHaveBeenCalledTimes(1);
        expect(next).toHaveBeenCalledTimes(2);
        expect(next.mock.calls[1][0].headers.get('Authorization')).toBe('Bearer access-2');
    });

    it('does not retry infinitely (401 twice triggers only one refresh)', async () => {
        coordinator.ensureFreshAccessToken.mockResolvedValue('access-1');
        coordinator.forceRefresh.mockResolvedValue('access-2');

        const req = new HttpRequest('GET', 'https://api.example/jobs');

        const next = vi.fn(() =>
            throwError(() => new HttpErrorResponse({ status: 401, url: req.url, statusText: 'Unauthorized' })),
        );

        await expect(
            firstValueFrom(TestBed.runInInjectionContext(() => authRefreshInterceptor(req, next))),
        ).rejects.toBeInstanceOf(HttpErrorResponse);

        expect(coordinator.forceRefresh).toHaveBeenCalledTimes(1);
        expect(next).toHaveBeenCalledTimes(2);
    });

    it('navigates to /login and rethrows when refresh fails on 401', async () => {
        coordinator.ensureFreshAccessToken.mockResolvedValue('access-1');
        coordinator.forceRefresh.mockResolvedValue(null);

        const req = new HttpRequest('GET', 'https://api.example/jobs');
        const next = vi.fn(() =>
            throwError(() => new HttpErrorResponse({ status: 401, url: req.url, statusText: 'Unauthorized' })),
        );

        await expect(
            firstValueFrom(TestBed.runInInjectionContext(() => authRefreshInterceptor(req, next))),
        ).rejects.toBeInstanceOf(HttpErrorResponse);

        expect(router.navigate).toHaveBeenCalledWith(['/login'], { queryParams: { returnUrl: '/jobs' } });
    });
});
