import { provideZonelessChangeDetection } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';import { TenantContext } from './tenant-context';

describe('TenantContext', () => {
  let component: TenantContext;
  let fixture: ComponentFixture<TenantContext>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [TenantContext],
      providers: [provideZonelessChangeDetection()],
    })
    .compileComponents();

    fixture = TestBed.createComponent(TenantContext);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
