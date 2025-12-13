import { ComponentFixture, TestBed } from '@angular/core/testing';

import { SchedulesPage } from './schedules-page';

describe('SchedulesPage', () => {
  let component: SchedulesPage;
  let fixture: ComponentFixture<SchedulesPage>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [SchedulesPage]
    })
    .compileComponents();

    fixture = TestBed.createComponent(SchedulesPage);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
