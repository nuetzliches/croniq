import { DialogRef, DIALOG_DATA } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Field, form, required, submit } from '@angular/forms/signals';
import { CalendarRuleDefinitionLooseSchema } from '@croniq/api-schema';
import { CqDialogComponent, CqDialogFooterDirective, CqDialogHeaderDirective } from 'ui-kit';
import type { CalendarMode, CalendarRuleDefinition, CroniqCalendarSeedDefinition } from '@croniq/api-schema';

type CalendarDialogData = CroniqCalendarSeedDefinition | null;

type RulesParseResult = {
    rules: CalendarRuleDefinition[];
    error: string | null;
};

interface CalendarFormModel {
    calendarId: string;
    name: string;
    description: string;
    timeZoneId: string;
    mode: string;
    enabled: boolean;
    rulesJson: string;
}

const DEFAULT_TIME_ZONE = 'UTC';
const DEFAULT_MODE: CalendarMode = 0;
const MODE_OPTIONS: ReadonlyArray<{ value: string; label: string }> = [
    { value: '0', label: 'Include' },
    { value: '1', label: 'Exclude' },
];

function mapToFormModel(data: CalendarDialogData): CalendarFormModel {
    const calendarId =
        typeof data?.calendarId === 'string' ? data.calendarId.trim() : '';
    const name = typeof data?.name === 'string' ? data.name.trim() : '';
    const description = typeof data?.description === 'string' ? data.description.trim() : '';
    const timeZoneId =
        typeof data?.timeZoneId === 'string' && data.timeZoneId.trim()
            ? data.timeZoneId.trim()
            : DEFAULT_TIME_ZONE;

    return {
        calendarId,
        name,
        description,
        timeZoneId,
        mode: String(normalizeCalendarMode(data?.mode)),
        enabled: data?.enabled ?? true,
        rulesJson: formatRulesJson(data?.rules),
    };
}

function formatRulesJson(rules: CalendarRuleDefinition[] | null | undefined): string {
    if (!Array.isArray(rules) || rules.length === 0) {
        return '';
    }
    return JSON.stringify(rules, null, 2);
}

function parseRulesJson(value: string): RulesParseResult {
    const trimmed = value.trim();
    if (!trimmed) {
        return { rules: [], error: null };
    }

    let parsed: unknown;
    try {
        parsed = JSON.parse(trimmed);
    } catch {
        return { rules: [], error: 'Rules must be valid JSON.' };
    }

    if (!Array.isArray(parsed)) {
        return { rules: [], error: 'Rules must be a JSON array.' };
    }

    const result = CalendarRuleDefinitionLooseSchema.array().safeParse(parsed);
    if (!result.success) {
        return { rules: [], error: 'Rules JSON does not match the expected schema.' };
    }
    const normalized = result.data.map((rule) => ({
        ...rule,
        dailyWindow: rule.dailyWindow ?? undefined,
        weeklyWindow: rule.weeklyWindow ?? undefined,
        annualDateList: rule.annualDateList ?? undefined,
        dateList: rule.dateList ?? undefined,
        cronRule: rule.cronRule ?? undefined,
    }));

    return { rules: normalized, error: null };
}

function normalizeCalendarMode(value: unknown): CalendarMode {
    if (value === 1 || value === '1') {
        return 1;
    }
    return DEFAULT_MODE;
}

@Component({
    selector: 'cq-calendar-dialog',
    imports: [Field, CqDialogComponent, CqDialogHeaderDirective, CqDialogFooterDirective],
    templateUrl: './calendar-dialog.component.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class CalendarDialogComponent {
    private readonly dialogRef = inject(DialogRef);
    readonly data = inject<CalendarDialogData>(DIALOG_DATA);

    readonly isEdit = !!this.data?.calendarId;
    readonly submitAttempted = signal(false);

    readonly model = signal(mapToFormModel(this.data));

    readonly form = form(this.model, (f) => {
        required(f.calendarId, { message: 'Calendar ID is required.' });
        required(f.name, { message: 'Name is required.' });
        required(f.timeZoneId, { message: 'Time zone is required.' });
    });

    readonly calendarIdInvalid = computed(() => !this.model().calendarId.trim());
    readonly nameInvalid = computed(() => !this.model().name.trim());
    readonly timeZoneInvalid = computed(() => !this.model().timeZoneId.trim());

    readonly rulesParseResult = computed(() => parseRulesJson(this.model().rulesJson));
    readonly rulesError = computed(() => this.rulesParseResult().error);

    readonly modeOptions = MODE_OPTIONS;

    close(): void {
        this.dialogRef.close();
    }

    async onSubmit(event: SubmitEvent) {
        event.preventDefault();
        this.submitAttempted.set(true);

        await submit(this.form, async () => {
            const rulesResult = this.rulesParseResult();
            if (rulesResult.error) {
                return;
            }

            const model = this.model();
            const payload: CroniqCalendarSeedDefinition = {
                calendarId: model.calendarId.trim(),
                name: model.name.trim(),
                description: model.description.trim() ? model.description.trim() : null,
                timeZoneId: model.timeZoneId.trim(),
                mode: normalizeCalendarMode(model.mode),
                enabled: model.enabled === false ? false : true,
                rules: rulesResult.rules.length ? rulesResult.rules : null,
            };

            this.dialogRef.close(payload);
        });
    }
}
