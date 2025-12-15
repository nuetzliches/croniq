import SwaggerParser from '@apidevtools/swagger-parser';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import http from 'node:http';
import https from 'node:https';
import { dirname, isAbsolute, join, resolve } from 'node:path';

import { generateZodClientFromOpenAPI } from 'openapi-zod-client';
import type { OpenAPIObject, PathItemObject } from 'openapi3-ts';
import prettier, { type Options as PrettierOptions } from 'prettier';
import ts from 'typescript';

import generatorConfig, { type SchemaGenerationConfig } from '../config/openapi-zod-client.config';

async function main(): Promise<void> {
    const configs = normalizeConfig(generatorConfig);

    for (const config of configs) {
        const openApiDoc = await loadOpenApiDocument(config.input);

        if (config.mode === 'split') {
            await generateGroupedOutput(config, openApiDoc);
            continue;
        }

        const distPath = resolvePath(config.output);
        const templatePath = config.template ? resolvePath(config.template) : undefined;

        await ensureOutputDirectory(distPath);

        await generateZodClientFromOpenAPI({
            openApiDoc,
            distPath,
            templatePath,
            prettierConfig: config.prettier ?? undefined,
            options: config.options,
        });

        await postProcessSchemaFile(distPath, config.prettier ?? null);

        console.log(`✓ Generated schemas at ${distPath}`);
    }
}

async function generateGroupedOutput(
    config: Extract<SchemaGenerationConfig, { mode: 'split' }>,
    openApiDoc: OpenAPIObject,
): Promise<void> {
    const outputDir = resolvePath(config.output);
    await resetDirectory(outputDir);

    const templatePath = resolvePath(config.template);
    const partitions = config.groupBy === 'tag'
        ? partitionDocumentByPrimaryTag(openApiDoc)
        : partitionDocumentByPathSegment(openApiDoc);

    if (partitions.length === 0) {
        console.warn('No groups detected in the OpenAPI document; skipping split generation.');
        return;
    }

    const indexEntries: Array<{ exportName: string; fileBase: string }> = [];

    for (const { normalized, document, originalName } of partitions) {
        const fileBase = normalized;
        const distPath = join(outputDir, `${fileBase}.ts`);
        const apiClientName = `${pascalCase(originalName)}Api`;

        await generateZodClientFromOpenAPI({
            openApiDoc: document,
            distPath,
            templatePath,
            prettierConfig: config.prettier ?? undefined,
            options: {
                ...config.options,
                groupStrategy: 'none',
                apiClientName,
            },
        });

        indexEntries.push({ exportName: apiClientName, fileBase });
        console.log(`✓ Generated ${apiClientName} endpoints at ${distPath}`);
    }

    const indexPath = join(outputDir, 'index.ts');
    const indexSource = indexEntries
        .map(({ exportName, fileBase }) => `export { ${exportName} } from './${fileBase}';`)
        .join('\n')
        .concat(indexEntries.length ? '\n' : '');

    await writeFile(indexPath, indexSource, 'utf8');
    console.log(`✓ Wrote endpoint index at ${indexPath}`);
}

function normalizeConfig(
    config: SchemaGenerationConfig | SchemaGenerationConfig[],
): SchemaGenerationConfig[] {
    return Array.isArray(config) ? config : [config];
}

async function loadOpenApiDocument(input: string): Promise<OpenAPIObject> {
    const target = resolveInput(input);
    const document = isHttpUrl(target)
        ? (await fetchJson(target)) as OpenAPIObject
        : (await SwaggerParser.parse(target)) as OpenAPIObject;

    if (!document?.openapi?.startsWith('3.')) {
        throw new Error(
            `Unsupported OpenAPI version (${document?.openapi ?? 'unknown'}). Please provide a v3 document.`,
        );
    }

    return document;
}

async function fetchJson(url: string): Promise<unknown> {
    const client = url.startsWith('https://') ? https : http;

    return new Promise((resolvePromise, reject) => {
        const request = client.get(url, (response) => {
            const status = response.statusCode ?? 0;
            if (status < 200 || status >= 300) {
                response.resume();
                reject(new Error(`Failed to fetch OpenAPI document (${status}) from ${url}`));
                return;
            }

            response.setEncoding('utf8');
            let payload = '';
            response.on('data', (chunk) => {
                payload += chunk;
            });
            response.on('end', () => {
                try {
                    resolvePromise(JSON.parse(payload));
                } catch (error) {
                    reject(
                        new Error(
                            `OpenAPI endpoint did not return valid JSON (${url}): ${String(
                                (error as Error).message ?? error,
                            )}`,
                        ),
                    );
                }
            });
        });

        request.on('error', (error) => {
            reject(error);
        });
    });
}

async function ensureOutputDirectory(targetFile: string): Promise<void> {
    await mkdir(dirname(targetFile), { recursive: true });
}

async function resetDirectory(targetDir: string): Promise<void> {
    await rm(targetDir, { force: true, recursive: true });
    await mkdir(targetDir, { recursive: true });
}

function resolveInput(input: string): string {
    if (isHttpUrl(input)) {
        return input;
    }

    return resolvePath(input);
}

function resolvePath(target: string): string {
    return isAbsolute(target) ? target : resolve(process.cwd(), target);
}

function isHttpUrl(value: string): boolean {
    return /^https?:\/\//i.test(value);
}

type GroupPartition = {
    normalized: string;
    originalName: string;
    document: OpenAPIObject;
};

function partitionDocumentByPrimaryTag(openApiDoc: OpenAPIObject): GroupPartition[] {
    const groups = new Map<string, GroupPartition>();
    const methods: Array<keyof PathItemObject> = ['get', 'put', 'post', 'delete', 'patch', 'options', 'head', 'trace'];
    const paths = openApiDoc.paths ?? {};

    for (const [pathKey, pathItem] of Object.entries(paths)) {
        if (!pathItem) continue;

        for (const method of methods) {
            const operation = pathItem[method];
            if (!operation) continue;

            const tagName = operation.tags?.[0] ?? 'default';
            const normalized = normalizeName(tagName);

            if (!groups.has(normalized)) {
                groups.set(normalized, {
                    normalized,
                    originalName: tagName,
                    document: createDocumentSkeleton(openApiDoc),
                });
            }

            const group = groups.get(normalized)!;
            if (!group.document.paths) {
                group.document.paths = {};
            }

            if (!group.document.paths[pathKey]) {
                group.document.paths[pathKey] = copyPathMetadata(pathItem);
            }

            group.document.paths[pathKey]![method] = clone(operation);
        }
    }

    return Array.from(groups.values());
}

function partitionDocumentByPathSegment(openApiDoc: OpenAPIObject): GroupPartition[] {
    const groups = new Map<string, GroupPartition>();
    const methods: Array<keyof PathItemObject> = ['get', 'put', 'post', 'delete', 'patch', 'options', 'head', 'trace'];
    const paths = openApiDoc.paths ?? {};

    for (const [pathKey, pathItem] of Object.entries(paths)) {
        if (!pathItem) continue;

        const segment = getPrimarySegment(pathKey);
        const normalized = normalizeName(segment);

        if (!groups.has(normalized)) {
            groups.set(normalized, {
                normalized,
                originalName: segment,
                document: createDocumentSkeleton(openApiDoc),
            });
        }

        const group = groups.get(normalized)!;
        if (!group.document.paths) {
            group.document.paths = {};
        }

        if (!group.document.paths[pathKey]) {
            group.document.paths[pathKey] = copyPathMetadata(pathItem);
        }

        for (const method of methods) {
            const operation = pathItem[method];
            if (!operation) continue;
            group.document.paths[pathKey]![method] = clone(operation);
        }
    }

    return Array.from(groups.values());
}

function createDocumentSkeleton(source: OpenAPIObject): OpenAPIObject {
    return {
        ...source,
        paths: {},
    };
}

function copyPathMetadata(pathItem: PathItemObject): PathItemObject {
    const { summary, description, servers, parameters } = pathItem;
    return {
        summary,
        description,
        servers: servers ? clone(servers) : undefined,
        parameters: parameters ? clone(parameters) : undefined,
    };
}

function getPrimarySegment(pathKey: string): string {
    const cleaned = pathKey.replace(/^\/+/, '');
    const [segment] = cleaned.split('/');
    return segment && segment !== '' ? segment : 'root';
}

function normalizeName(rawName: string): string {
    return rawName
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, '-')
        .replace(/^-+|-+$/g, '')
        .replace(/--+/g, '-')
        .trim() || 'default';
}

function pascalCase(value: string): string {
    const parts = value
        .trim()
        .split(/[^a-zA-Z0-9]+/g)
        .filter(Boolean);
    if (parts.length === 0) return 'Default';
    return parts.map((part) => part.charAt(0).toUpperCase() + part.slice(1)).join('');
}

function clone<T>(value: T): T {
    if (typeof structuredClone === 'function') {
        return structuredClone(value);
    }

    return JSON.parse(JSON.stringify(value)) as T;
}

async function postProcessSchemaFile(distPath: string, prettierConfig: PrettierOptions | null): Promise<void> {
    const originalSource = await readFile(distPath, 'utf8');
    const { code: transformedSource, changed } = ensureRecordHasKeyArgument(originalSource);

    if (!changed) {
        return;
    }

    const formatted = prettierConfig
        ? prettier.format(transformedSource, { ...prettierConfig, parser: prettierConfig.parser ?? 'typescript' })
        : transformedSource;

    await writeFile(distPath, formatted, 'utf8');
}

function ensureRecordHasKeyArgument(source: string): { code: string; changed: boolean } {
    let mutated = false;
    const sourceFile = ts.createSourceFile('schemas.ts', source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);

    const transformer: ts.TransformerFactory<ts.SourceFile> = (context) => {
        const visit: ts.Visitor = (node) => {
            if (
                ts.isCallExpression(node) &&
                ts.isPropertyAccessExpression(node.expression) &&
                ts.isIdentifier(node.expression.expression) &&
                node.expression.expression.text === 'z' &&
                node.expression.name.text === 'record' &&
                node.arguments.length === 1
            ) {
                mutated = true;

                const keyTypeCall = ts.factory.createCallExpression(
                    ts.factory.createPropertyAccessExpression(ts.factory.createIdentifier('z'), 'string'),
                    undefined,
                    [],
                );

                return ts.factory.updateCallExpression(node, node.expression, node.typeArguments, [
                    keyTypeCall,
                    node.arguments[0],
                ]);
            }

            return ts.visitEachChild(node, visit, context);
        };

        return (node) => ts.visitNode(node, visit);
    };

    const result = ts.transform(sourceFile, [transformer]);
    const transformedFile = result.transformed[0] as ts.SourceFile;
    const printer = ts.createPrinter({ newLine: ts.NewLineKind.LineFeed });
    const output = printer.printFile(transformedFile);
    result.dispose();

    return { code: output, changed: mutated };
}

main().catch((error) => {
    console.error('Failed to generate Zod schemas from OpenAPI', error);
    process.exitCode = 1;
});
