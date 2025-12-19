import { TestBed } from '@angular/core/testing';
import { CRONIQ_API_BASE_URL, CRONIQ_CREDENTIAL_SUPPLIER } from './api-client';

describe('data-access api-client', () => {
    beforeEach(() => {
        TestBed.configureTestingModule({});
    });

    it('provides a default API base URL', () => {
        const baseUrl = TestBed.inject(CRONIQ_API_BASE_URL);
        expect(baseUrl).toBeTruthy();
    });

    it('defaults credential supplier to null', () => {
        const supplier = TestBed.inject(CRONIQ_CREDENTIAL_SUPPLIER);
        expect(supplier).toBeNull();
    });
});
