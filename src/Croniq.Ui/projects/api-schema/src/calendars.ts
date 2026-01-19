import { z } from 'zod';
import type { CalendarResponse, CalendarRuleDefinition } from '../generated/schemas';
import { CalendarAnnualDateListRule, CalendarCronRule, CalendarDailyWindowRule, CalendarDateListRule, CalendarResponse as CalendarResponseSchema, CalendarRuleDefinition as CalendarRuleDefinitionSchema, CalendarWeeklyWindowRule } from '../generated/schemas';

const optionalNullable = <T extends z.ZodTypeAny>(schema: T) => schema.nullable().optional();

export const CalendarRuleDefinitionLooseSchema: z.ZodType<CalendarRuleDefinition> =
    CalendarRuleDefinitionSchema.extend({
        dailyWindow: optionalNullable(CalendarDailyWindowRule),
        weeklyWindow: optionalNullable(CalendarWeeklyWindowRule),
        annualDateList: optionalNullable(CalendarAnnualDateListRule),
        dateList: optionalNullable(CalendarDateListRule),
        cronRule: optionalNullable(CalendarCronRule),
    }).transform((rule) => ({
        ...rule,
        dailyWindow: rule.dailyWindow ?? undefined,
        weeklyWindow: rule.weeklyWindow ?? undefined,
        annualDateList: rule.annualDateList ?? undefined,
        dateList: rule.dateList ?? undefined,
        cronRule: rule.cronRule ?? undefined,
    }));

export const CalendarResponseLooseSchema: z.ZodType<CalendarResponse> = CalendarResponseSchema.extend({
    rules: z.array(CalendarRuleDefinitionLooseSchema).nullable().optional(),
});
