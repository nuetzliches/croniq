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
