import { ComponentFixture, TestBed } from '@angular/core/testing';

import { StatusBeacon } from './status-beacon';

describe('StatusBeacon', () => {
  let component: StatusBeacon;
  let fixture: ComponentFixture<StatusBeacon>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [StatusBeacon]
    })
    .compileComponents();

    fixture = TestBed.createComponent(StatusBeacon);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
