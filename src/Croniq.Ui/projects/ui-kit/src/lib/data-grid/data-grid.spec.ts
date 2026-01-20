import { provideZonelessChangeDetection } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { Component, TemplateRef, ViewChild } from '@angular/core';
import { By } from '@angular/platform-browser';
import { CdkVirtualScrollViewport } from '@angular/cdk/scrolling';
import { DataGrid, ColumnDef, ColumnCellContext, ColumnHeaderContext } from './data-grid';

type Row = { id: string; name: string; status: 'active' | 'paused' };

@Component({
    imports: [DataGrid],
    template: `
    <ng-template #nameHeader let-column="column">
      {{ column.id }}
    </ng-template>

    <ng-template #nameCell let-row let-column="column">
      {{ row.name }}-{{ column.id }}
    </ng-template>

    <ng-template #statusCell let-row>
      {{ row.status }}
    </ng-template>

    <cq-data-grid
      [rows]="rows"
      [rowKey]="rowKey"
      [columns]="columns"
    />
  `,
    styles: [`
    :host {
      display: block;
      height: 240px;
    }

    cq-data-grid {
      height: 100%;
    }
  `],
})
class HostComponent {
    @ViewChild('nameHeader', { static: true })
    nameHeader!: TemplateRef<ColumnHeaderContext<Row>>;

    @ViewChild('nameCell', { static: true })
    nameCell!: TemplateRef<ColumnCellContext<Row>>;

    @ViewChild('statusCell', { static: true })
    statusCell!: TemplateRef<ColumnCellContext<Row>>;

    rows: readonly Row[] = [
        { id: 'r1', name: 'Alpha', status: 'active' },
        { id: 'r2', name: 'Beta', status: 'paused' },
    ];

    get columns(): readonly ColumnDef<Row>[] {
        return [
            { id: 'name', header: this.nameHeader, cell: this.nameCell },
            { id: 'status', header: this.nameHeader, cell: this.statusCell },
        ];
    }

    rowKey = (row: Row) => row.id;
}

describe('DataGrid', () => {
    let fixture: ComponentFixture<HostComponent>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [HostComponent],
            providers: [provideZonelessChangeDetection()],
        }).compileComponents();

        fixture = TestBed.createComponent(HostComponent);
        fixture.detectChanges();
        await fixture.whenStable();
        const viewportElement = fixture.nativeElement.querySelector('cdk-virtual-scroll-viewport') as HTMLElement | null;
        if (viewportElement) {
            Object.defineProperty(viewportElement, 'clientHeight', { value: 240, configurable: true });
            Object.defineProperty(viewportElement, 'clientWidth', { value: 800, configurable: true });
        }
        const viewport = fixture.debugElement.query(By.directive(CdkVirtualScrollViewport))?.componentInstance;
        viewport?.checkViewportSize();
        if (viewport) {
            viewport.setRenderedRange({ start: 0, end: fixture.componentInstance.rows.length });
            viewport.setRenderedContentOffset(0);
            await Promise.resolve();
        }
        fixture.detectChanges();
    });

    it('renders rows and column templates', () => {
        const rows = fixture.nativeElement.querySelectorAll('[role="row"]');

        expect(rows.length).toBe(3); // header + 2 data rows
        expect(fixture.nativeElement.textContent).toContain('Alpha-name');
        expect(fixture.nativeElement.textContent).toContain('Beta-name');
        expect(fixture.nativeElement.textContent).toContain('active');
        expect(fixture.nativeElement.textContent).toContain('paused');
    });
});
