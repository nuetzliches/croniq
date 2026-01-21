import { HttpClient } from '@angular/common/http';
import type { EndpointDefinition, ParameterLocation } from '@croniq/api-schema';
import { Observable, map } from 'rxjs';
import { z } from 'zod';
import type { CallerContext, CroniqCredentialSupplier } from './api-client.types';

type AnyZodSchema = z.ZodType<unknown>;
export type BaseUrlResolver = string | (() => string);

export interface EndpointCallConfig {
    path?: Record<string, unknown>;
    query?: Record<string, unknown>;
    headers?: Record<string, unknown>;
    body?: unknown;
    context?: CallerContext;
    responseType?: 'json' | 'text';
    responseSchema?: AnyZodSchema | null;
    parseResponse?: boolean;
    sessionToken?: string | null;
}

export class EndpointExecutor {
    constructor(
        private readonly http: HttpClient,
        private readonly baseUrl: BaseUrlResolver,
        private readonly clientId = 'Croniq.Ui',
        private readonly credentials?: CroniqCredentialSupplier | null,
    ) { }

    execute$<T = unknown>(endpoint: EndpointDefinition, config: EndpointCallConfig = {}): Observable<T> {
        const pathValues = this.normalizeParams(endpoint, 'Path', config.path);
        const queryValues = this.normalizeParams(endpoint, 'Query', config.query);
        const headerValues = this.normalizeParams(endpoint, 'Header', config.headers);
        const body = this.normalizeBody(endpoint, config.body);
        const responseType = config.responseType ?? 'json';
        const url = this.buildUrl(endpoint.path, pathValues);
        const headers = this.createHeaders(config.context, headerValues, config);
        const params = this.createQueryParams(queryValues);

        const baseOptions: {
            headers: Record<string, string>;
            params?: Record<string, string>;
            body?: unknown;
        } = { headers };

        if (params) {
            baseOptions.params = params;
        }
        if (body !== undefined) {
            baseOptions.body = body;
        }

        const method = endpoint.method.toUpperCase();
        const request$ = this.http.request(method, url, {
            ...baseOptions,
            responseType: responseType === 'text' ? 'text' : 'json',
            observe: 'body',
        } as unknown as { responseType: 'json' | 'text'; observe: 'body' });

        return request$.pipe(map((response) => this.parseResponse<T>(endpoint, response, config)));
    }

    private normalizeParams(
        endpoint: EndpointDefinition,
        location: ParameterLocation,
        provided?: Record<string, unknown>,
    ): Record<string, string> {
        const definitions = (endpoint.parameters ?? []).filter((param) => param.type === location);
        if (!definitions.length) {
            return {};
        }
        const source = provided ?? {};
        return definitions.reduce<Record<string, string>>((acc, param) => {
            const value = source[param.name];
            if (value === undefined || value === null) {
                if (location === 'Path') {
                    throw new Error(
                        `Missing required path parameter "${param.name}" for ${endpoint.method.toUpperCase()} ${endpoint.path}`,
                    );
                }
                return acc;
            }
            acc[param.name] = String(value);
            return acc;
        }, {});
    }

    private normalizeBody(endpoint: EndpointDefinition, body: unknown): unknown {
        const definition = (endpoint.parameters ?? []).find((param) => param.type === 'Body');
        if (!definition) {
            return undefined;
        }
        if (body === undefined || body === null) {
            throw new Error(`Missing body for ${endpoint.method.toUpperCase()} ${endpoint.path}`);
        }
        return this.parseWithSchema(definition.schema, body);
    }

    private createHeaders(
        context?: CallerContext,
        extras?: Record<string, string>,
        auth?: Pick<EndpointCallConfig, 'sessionToken'>,
    ): Record<string, string> {
        const headers: Record<string, string> = {
            'X-Croniq-Client': this.clientId,
            'Cache-Control': 'no-store, no-cache, max-age=0',
            Pragma: 'no-cache',
        };

        if (context?.source) {
            headers['X-Croniq-Source'] = context.source;
        }
        if (context?.actor) {
            headers['X-Croniq-Actor'] = context.actor;
        }
        if (context?.tenantId) {
            headers['X-Croniq-Tenant'] = context.tenantId;
        }
        if (context?.environment) {
            headers['X-Croniq-Environment'] = context.environment;
        }
        if (context?.command) {
            headers['X-Croniq-Command'] = context.command;
        }

        const resolvedSessionToken = auth?.sessionToken ?? this.credentials?.getSessionToken() ?? null;
        if (resolvedSessionToken) {
            headers['Authorization'] = `Bearer ${resolvedSessionToken}`;
        }

        if (extras) {
            Object.entries(extras).forEach(([key, value]) => {
                headers[key] = value;
            });
        }

        return headers;
    }

    private createQueryParams(values?: Record<string, string>): Record<string, string> | undefined {
        if (!values) {
            return undefined;
        }
        const entries = Object.entries(values).filter(([, value]) => value !== undefined && value !== '');
        if (!entries.length) {
            return undefined;
        }
        return entries.reduce<Record<string, string>>((acc, [key, value]) => {
            acc[key] = value;
            return acc;
        }, {});
    }

    private buildUrl(template: string, values: Record<string, string>): string {
        const path = template.replace(/:([A-Za-z0-9_]+)/g, (_, key: string) => {
            if (!(key in values)) {
                throw new Error(`Missing value for path parameter ":${key}" in ${template}`);
            }
            return encodeURIComponent(values[key]);
        });
        const baseUrl = resolveBaseUrl(this.baseUrl);
        return `${baseUrl}${path}`;
    }

    private parseResponse<T>(endpoint: EndpointDefinition, payload: unknown, config: EndpointCallConfig): T {
        const schema = config.responseSchema ?? endpoint.response;
        const shouldParse = config.parseResponse ?? true;
        if (!shouldParse || !isZodSchema(schema) || schema instanceof z.ZodVoid) {
            return payload as T;
        }
        return schema.parse(payload) as T;
    }

    private parseWithSchema(schema: unknown, payload: unknown): unknown {
        return isZodSchema(schema) ? schema.parse(payload) : payload;
    }
}

export function requireEndpoint(
    endpoints: ReadonlyArray<EndpointDefinition>,
    method: EndpointDefinition['method'],
    path: string,
): EndpointDefinition {
    const target = endpoints.find((entry) => entry.method === method && entry.path === path);
    if (!target) {
        throw new Error(`Endpoint ${method.toUpperCase()} ${path} not found in provided catalog.`);
    }
    return target;
}

function isZodSchema(value: unknown): value is AnyZodSchema {
    return Boolean(value && typeof (value as { parse?: unknown }).parse === 'function');
}

function resolveBaseUrl(baseUrl: BaseUrlResolver): string {
    const resolved = typeof baseUrl === 'function' ? baseUrl() : baseUrl;
    return resolved?.trim() ?? '';
}
