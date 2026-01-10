import { ComponentFixture, TestBed } from '@angular/core/testing';
import { WebhookDialogComponent } from './webhook-dialog.component';
import { DialogRef, DIALOG_DATA } from '@angular/cdk/dialog';
import { vi, describe, it, expect, beforeEach } from 'vitest';

const dialogData = {
    endpoint: null,
    capabilities: {
        allowUnsignedHooks: true,
        defaultRequestsPerMinute: 60,
    },
};

describe('WebhookDialogComponent', () => {
    let component: WebhookDialogComponent;
    let fixture: ComponentFixture<WebhookDialogComponent>;
    type DialogRefMock = Pick<DialogRef<unknown>, 'close'>;
    let dialogRefMock: DialogRefMock;

    beforeEach(async () => {
        dialogRefMock = { close: vi.fn() as DialogRefMock['close'] };

        await TestBed.configureTestingModule({
            imports: [WebhookDialogComponent],
            providers: [
                { provide: DialogRef, useValue: dialogRefMock },
                { provide: DIALOG_DATA, useValue: dialogData },
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

    it('should validate required fields', async () => {
        await component.onSubmit(new SubmitEvent('submit'));
        expect(component.submitAttempted()).toBe(true);
        expect(dialogRefMock.close).not.toHaveBeenCalled();
    });

    it('should close with data when valid', async () => {
        component.webhookModel.update(m => ({ ...m, hookKey: 'test-hook', jobKey: 'test-job', secret: 'my-secret' }));
        fixture.detectChanges();

        await component.onSubmit(new SubmitEvent('submit'));

        expect(dialogRefMock.close).toHaveBeenCalledWith(expect.objectContaining({
            hookKey: 'test-hook',
            jobKey: 'test-job',
            secret: 'my-secret'
        }));
    });

    it('should convert string requestsPerMinute to number', async () => {
        component.webhookModel.update(m => ({
            ...m,
            hookKey: 'test-hook',
            jobKey: 'test-job',
            secret: 'my-secret',
            requestsPerMinute: '60' as unknown as number
        }));
        fixture.detectChanges();

        await component.onSubmit(new SubmitEvent('submit'));

        expect(dialogRefMock.close).toHaveBeenCalledWith(expect.objectContaining({
            requestsPerMinute: 60
        }));
    });

    it('should handle empty string requestsPerMinute as null', async () => {
        component.webhookModel.update(m => ({
            ...m,
            hookKey: 'test-hook',
            jobKey: 'test-job',
            secret: 'my-secret',
            requestsPerMinute: '' as unknown as number
        }));
        fixture.detectChanges();

        await component.onSubmit(new SubmitEvent('submit'));

        expect(dialogRefMock.close).toHaveBeenCalledWith(expect.objectContaining({
            requestsPerMinute: null
        }));
    });
});
