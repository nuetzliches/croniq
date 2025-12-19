import { z } from 'zod';
import type { EndpointDefinition } from '../schemas';
import {
    PasswordLoginRequest,
    PasswordRefreshRequest,
    PasswordLogoutRequest,
    PasswordChangePasswordRequest,
    CreateWebhookIpRuleRequest,
    ExecutionStatus,
    IssueApiKeyRequest,
    IssueTokenRequest,
    RotateWebhookSecretRequest,
    TriggerJobRequest,
    UpsertApiClientRequest,
    UpsertJobRequest,
    UpsertScheduleRequest,
    UpsertTenantRequest,
    UpsertWebhookEndpointRequest,
} from '../schemas';

export const AuthApi: EndpointDefinition[] = [
    {
        method: 'post',
        path: '/auth/change-password',
        description: `Changes the password for the currently authenticated password user. Requires a valid access token.`,
        requestFormat: 'json',
        parameters: [
            {
                name: 'body',
                type: 'Body',
                schema: PasswordChangePasswordRequest,
            },
        ],
        response: z.void(),
    },
    {
        method: 'post',
        path: '/auth/login',
        description: `Authenticates a username/password and issues access + refresh tokens. Tenant can be provided via tenantReference; it can be omitted if a default tenant is configured.`,
        requestFormat: 'json',
        parameters: [
            { name: 'body', type: 'Body', schema: PasswordLoginRequest },
        ],
        response: z.void(),
    },
    {
        method: 'post',
        path: '/auth/logout',
        description: `Revokes the provided refresh token.`,
        requestFormat: 'json',
        parameters: [
            { name: 'body', type: 'Body', schema: PasswordLogoutRequest },
        ],
        response: z.void(),
    },
    {
        method: 'post',
        path: '/auth/refresh',
        description: `Rotates the refresh token and returns a new access token.`,
        requestFormat: 'json',
        parameters: [
            { name: 'body', type: 'Body', schema: PasswordRefreshRequest },
        ],
        response: z.void(),
    },
] as const;

export type AuthApiEndpoint = (typeof AuthApi)[number];
