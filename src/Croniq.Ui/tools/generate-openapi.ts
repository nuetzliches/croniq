import { mkdirSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

import { OpenAPIRegistry, OpenApiGeneratorV31, extendZodWithOpenApi } from '@asteasolutions/zod-to-openapi';
import { z } from 'zod';

extendZodWithOpenApi(z);

type DomainRegistrar = (registry: OpenAPIRegistry) => void;

async function generateDocument(): Promise<void> {
    const registry = new OpenAPIRegistry();

    const registrars = await loadDomainRegistrars();
    for (const registrar of registrars) {
        registrar(registry);
    }

    const generator = new OpenApiGeneratorV31(registry.definitions);

    const document = generator.generateDocument({
        openapi: '3.1.0',
        info: {
            title: 'Croniq Admin Mock API',
            version: '0.1.0',
            description:
                'Generated from Zod schemas to keep UI models and OpenAPI docs in lockstep during development.',
        },
        servers: [{ url: 'https://api.croniq.dev' }],
        security: [{ 'X-Croniq-Key': [] }],
    });

    const outputPath = resolve('public/swagger.json');
    mkdirSync(dirname(outputPath), { recursive: true });
    writeFileSync(outputPath, JSON.stringify(document, null, 2), 'utf-8');

    console.log(`OpenAPI schema written to ${outputPath}`);
}

async function loadDomainRegistrars(): Promise<ReadonlyArray<DomainRegistrar>> {
    const openapiDir = resolve('projects/api-schema/openapi');
    const entries = readdirSync(openapiDir, { withFileTypes: true });
    const registrars: DomainRegistrar[] = [];

    const files = entries
        .filter((entry) => entry.isFile() && entry.name.endsWith('.ts'))
        .map((entry) => entry.name)
        .sort();

    for (const file of files) {
        const moduleUrl = pathToFileURL(resolve(openapiDir, file)).href;
        const module = await import(moduleUrl);
        const registrar: DomainRegistrar | undefined = module.registerDomain ?? module.default;
        if (typeof registrar === 'function') {
            registrars.push(registrar);
        } else {
            console.warn(`Skipping ${file}: missing registerDomain export.`);
        }
    }

    return registrars;
}

generateDocument().catch((error) => {
    console.error('Failed to generate OpenAPI schema', error);
    process.exitCode = 1;
});
