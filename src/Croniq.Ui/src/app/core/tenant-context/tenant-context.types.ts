export type TenantEnvironment = 'dev' | 'staging' | 'production';

export type TenantContextState = {
    tenantId: string;
    tenantName: string;
    environment: TenantEnvironment;
    region: string;
    blueprintVersion: string;
    policyCount: number;
    lastAuditedAt: string;
    featureFlags: ReadonlyArray<string>;
    source: string;
};

export type TenantPreset = {
    id: string;
    label: string;
    tenantName: string;
    defaultEnvironment: TenantEnvironment;
    region: string;
    blueprintVersion: string;
    policyCount: number;
    featureFlags: ReadonlyArray<string>;
    source: string;
};
