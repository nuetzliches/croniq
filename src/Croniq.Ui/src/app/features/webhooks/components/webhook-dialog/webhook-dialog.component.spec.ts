import { ComponentFixture, TestBed } from '@angular/core/testing';
import { WebhookDialogComponent } from './webhook-dialog.component';
import { DialogRef, DIALOG_DATA } from '@angular/cdk/dialog';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { RuntimeConfigService } from '@core/runtime-config.service';

describe('WebhookDialogComponent', () => {
    let component: WebhookDialogComponent;
    let fixture: ComponentFixture<WebhookDialogComponent>;
    let dialogRefMock: { close: any };

    beforeEach(async () => {
        dialogRefMock = { close: vi.fn() };

        await TestBed.configureTestingModule({
            imports: [WebhookDialogComponent],
            providers: [
                { provide: DialogRef, useValue: dialogRefMock },
                { provide: DIALOG_DATA, useValue: null },
                { provide: RuntimeConfigService, useValue: { webhooksAllowUnsignedHooks: true } },
            ],
        }).compileComponents();

        fixture = TestBed.createComponent(WebhookDialogComponent);
        component = fixture.componentInstance;
        fixture.detectChanges();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });

    it('should initialize in create mode when no data provided', () => {
        expect(component.isEdit).toBe(false);
        expect(component.webhookModel().hookKey).toBe('');
    });

    it('should validate required fields', () => {
        component.save();
        expect(component.submitAttempted()).toBe(true);
        expect(dialogRefMock.close).not.toHaveBeenCalled();
    });

    it('should close with data when valid', () => {
        component.webhookModel.update(m => ({ ...m, hookKey: 'test-hook', jobKey: 'test-job', secret: 'my-secret' }));
        fixture.detectChanges();

        component.save();

        expect(dialogRefMock.close).toHaveBeenCalledWith(expect.objectContaining({
            hookKey: 'test-hook',
            jobKey: 'test-job',
            secret: 'my-secret'
        }));
    });

    it('should convert string requestsPerMinute to number', () => {
        component.webhookModel.update(m => ({
            ...m,
            hookKey: 'test-hook',
            jobKey: 'test-job',
            secret: 'my-secret',
            requestsPerMinute: '60' as any
        }));
        fixture.detectChanges();

        component.save();

        expect(dialogRefMock.close).toHaveBeenCalledWith(expect.objectContaining({
            requestsPerMinute: 60
        }));
    });

    it('should handle empty string requestsPerMinute as null', () => {
        component.webhookModel.update(m => ({
            ...m,
            hookKey: 'test-hook',
            jobKey: 'test-job',
            secret: 'my-secret',
            requestsPerMinute: '' as any
        }));
        fixture.detectChanges();

        component.save();

        expect(dialogRefMock.close).toHaveBeenCalledWith(expect.objectContaining({
            requestsPerMinute: null
        }));
    });
});
