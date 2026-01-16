import { provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { DashboardPage } from './dashboard-page';
import { DashboardStore } from './dashboard.store';

describe('DashboardPage', () => {
  let component: DashboardPage;
  let fixture: ComponentFixture<DashboardPage>;

  const mockDashboardStore = {
    loading: signal(false),
    metrics: signal([]),
    metricsDisplay: signal([]),
    recentFailures: signal([]),
    scheduleForecast: signal({
      windowMinutes: 60,
      bucketMinutes: 5,
      rangeLabel: 'Unavailable',
      buckets: [],
      summaries: [],
      totalCount: 0,
      maxBucketCount: 0,
      hasData: false,
    }),
    selectedForecastIndex: signal(null),
    selectedForecast: signal(null),
    misfireHeatmap: signal([]),
    selectForecastBucket: () => { },
  };

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [DashboardPage],
      providers: [
        provideZonelessChangeDetection(),
        provideRouter([]),
      ],
    })
      .overrideComponent(DashboardPage, {
        set: {
          providers: [
            { provide: DashboardStore, useValue: mockDashboardStore }
          ]
        }
      })
      .compileComponents();

    fixture = TestBed.createComponent(DashboardPage);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
