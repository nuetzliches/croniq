import { ComponentFixture, TestBed } from '@angular/core/testing';

import { JobsPage } from './jobs-page';

describe('JobsPage', () => {
  let component: JobsPage;
  let fixture: ComponentFixture<JobsPage>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [JobsPage]
    })
    .compileComponents();

    fixture = TestBed.createComponent(JobsPage);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
