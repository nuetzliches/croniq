import type { TemplateContextOptions } from 'openapi-zod-client';
import type { Options as PrettierOptions } from 'prettier';

const OPENAPI_URL = process.env.CRONIQ_OPENAPI_URL ?? 'http://localhost:5000/swagger/v1/swagger.json';
const OUTPUT_DIR = 'projects/api-schema/generated';
const OUTPUT_FILE = `${OUTPUT_DIR}/schemas.ts`;
const ENDPOINT_OUTPUT_DIR = `${OUTPUT_DIR}/endpoints`;
const SCHEMAS_TEMPLATE = 'tools/templates/angular-http-client.hbs';
const ENDPOINT_TEMPLATE = 'tools/templates/angular-domain-endpoints.hbs';

export type SchemaGenerationConfig =
    | {
        mode?: 'single';
        input: string;
        output: string;
        template?: string;
        prettier?: PrettierOptions | null;
        options?: TemplateContextOptions;
    }
    | {
        mode: 'split';
        groupBy: 'tag' | 'path';
        input: string;
        output: string;
        template: string;
        prettier?: PrettierOptions | null;
        options?: TemplateContextOptions;
    };

const sharedPrettierConfig: PrettierOptions = {
    parser: 'typescript',
    tabWidth: 4,
    singleQuote: true,
    trailingComma: 'all',
};

const config: SchemaGenerationConfig[] = [
    {
        mode: 'single',
        input: OPENAPI_URL,
        output: OUTPUT_FILE,
        template: SCHEMAS_TEMPLATE,
        prettier: sharedPrettierConfig,
        options: {
            groupStrategy: 'none',
            shouldExportAllSchemas: true,
            withAlias: false,
        },
    },
    {
        mode: 'split',
        groupBy: 'path',
        input: OPENAPI_URL,
        output: ENDPOINT_OUTPUT_DIR,
        template: ENDPOINT_TEMPLATE,
        prettier: sharedPrettierConfig,
        options: {
            groupStrategy: 'none',
            shouldExportAllSchemas: true,
            withAlias: false,
        },
    },
];

export default config;
