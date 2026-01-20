import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { firstValueFrom } from 'rxjs';
import { RuntimeConfigService } from './runtime-config.service';

describe('RuntimeConfigService', () => {
  let service: RuntimeConfigService;
  let http: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [provideHttpClient(), provideHttpClientTesting()],
    });

    service = TestBed.inject(RuntimeConfigService);
    http = TestBed.inject(HttpTestingController);
  });

  afterEach(() => {
    http.verify();
  });

  it('loads runtime config and trims apiBaseUrl', async () => {
    const loadPromise = firstValueFrom(service.load());

    const req = http.expectOne('assets/croniq-config.json');
    req.flush({
      apiBaseUrl: 'http://localhost:5080/',
      swaggerUiUrl: 'http://localhost:5080/swagger',
    });

    await loadPromise;

    expect(service.apiBaseUrl).toBe('http://localhost:5080');
    expect(service.swaggerUiUrl).toBe('http://localhost:5080/swagger');
  });

  it('builds swaggerUiUrl from apiBaseUrl when not provided', async () => {
    const loadPromise = firstValueFrom(service.load());

    const req = http.expectOne('assets/croniq-config.json');
    req.flush({
      apiBaseUrl: 'http://localhost:5080',
    });

    await loadPromise;

    expect(service.swaggerUiUrl).toBe('http://localhost:5080/swagger/index.html');
  });

  it('exposes defaultTenantId from runtime config', async () => {
    const loadPromise = firstValueFrom(service.load());

    const req = http.expectOne('assets/croniq-config.json');
    req.flush({
      defaultTenantId: ' tenant-a ',
    });

    await loadPromise;

    expect(service.defaultTenantId).toBe('tenant-a');
  });

  it('exposes webhooks activity stream settings', async () => {
    const loadPromise = firstValueFrom(service.load());

    const req = http.expectOne('assets/croniq-config.json');
    req.flush({
      apiBaseUrl: 'http://localhost:5080/',
      webhooks: {
        activityStream: {
          mode: 'sse',
          grpcBaseUrl: 'http://localhost:5082/',
          sseBaseUrl: '/streams/',
        },
      },
    });

    await loadPromise;

    expect(service.webhooksActivityStreamMode).toBe('sse');
    expect(service.webhooksActivityGrpcBaseUrl).toBe('http://localhost:5082');
    expect(service.webhooksActivitySseBaseUrl).toBe('/streams');
  });
});
