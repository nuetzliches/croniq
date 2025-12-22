export type TenantEnvironment = string;

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
