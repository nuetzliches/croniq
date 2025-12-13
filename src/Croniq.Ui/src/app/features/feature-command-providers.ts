import { Provider } from '@angular/core';

import { PRIMARY_NAV_COMMANDS_PROVIDER } from '../core/navigation/nav-items';
import { DASHBOARD_COMMANDS_PROVIDER } from './dashboard/dashboard.commands';
import { JOBS_COMMANDS_PROVIDER } from './jobs/jobs.commands';
import { SCHEDULES_COMMANDS_PROVIDER } from './schedules/schedules.commands';
import { TENANTS_COMMANDS_PROVIDER } from './tenants/tenants.commands';
import { WEBHOOKS_COMMANDS_PROVIDER } from './webhooks/webhooks.commands';

export const FEATURE_COMMAND_PROVIDERS: Provider[] = [
    PRIMARY_NAV_COMMANDS_PROVIDER,
    DASHBOARD_COMMANDS_PROVIDER,
    SCHEDULES_COMMANDS_PROVIDER,
    JOBS_COMMANDS_PROVIDER,
    WEBHOOKS_COMMANDS_PROVIDER,
    TENANTS_COMMANDS_PROVIDER,
];
