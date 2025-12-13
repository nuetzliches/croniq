import { ComponentFixture, TestBed } from '@angular/core/testing';

import { TenantsPage } from './tenants-page';

describe('TenantsPage', () => {
  let component: TenantsPage;
  let fixture: ComponentFixture<TenantsPage>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [TenantsPage]
    })
    .compileComponents();

    fixture = TestBed.createComponent(TenantsPage);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
