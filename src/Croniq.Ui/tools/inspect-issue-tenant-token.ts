type Args = {
    baseUrl: string;
    tenantId: string;
    environment?: string;
    clientId: string;
    ttlMinutes?: number;
    scopes?: string[];
    audience?: string;
};

function parseArgs(argv: string[]): Args {
    const args: Record<string, string> = {};
    for (const raw of argv) {
        const match = /^--([^=]+)=(.*)$/.exec(raw);
        if (match) {
            args[match[1]] = match[2];
        }
    }

    const baseUrl = args.baseUrl ?? process.env.CRONIQ_API_BASE_URL ?? 'http://localhost:5000';
    const tenantId = args.tenantId ?? 'cron-lab';
    const clientId = args.clientId ?? 'ui';

    if (!tenantId || !clientId) {
        throw new Error('Missing required args: --tenantId=... and --clientId=...');
    }

    return {
        baseUrl,
        tenantId,
        environment: args.environment,
        clientId,
        ttlMinutes: args.ttlMinutes ? Number(args.ttlMinutes) : undefined,
        scopes: args.scopes ? args.scopes.split(',').map((s) => s.trim()).filter(Boolean) : undefined,
        audience: args.audience,
    };
}

async function main(): Promise<void> {
    const parsed = parseArgs(process.argv.slice(2));

    const sessionToken = process.env.CRONIQ_SESSION_TOKEN ?? '';
    const apiKey = process.env.CRONIQ_API_KEY ?? '';

    if (!sessionToken && !apiKey) {
        console.error('Missing credentials: set CRONIQ_SESSION_TOKEN (Bearer) or CRONIQ_API_KEY (X-Croniq-Key).');
    }

    const url = new URL(`${parsed.baseUrl.replace(/\/$/, '')}/tenants/${encodeURIComponent(parsed.tenantId)}/tokens`);
    if (parsed.environment) {
        url.searchParams.set('environment', parsed.environment);
    }

    const headers: Record<string, string> = {
        'Content-Type': 'application/json',
        'X-Croniq-Client': 'Croniq.Ui',
    };

    if (sessionToken) {
        headers.Authorization = `Bearer ${sessionToken}`;
    }
    if (apiKey) {
        headers['X-Croniq-Key'] = apiKey;
    }

    const body = {
        clientId: parsed.clientId,
        scopes: parsed.scopes?.length ? parsed.scopes : null,
        audience: parsed.audience ?? null,
        ttlMinutes: typeof parsed.ttlMinutes === 'number' ? parsed.ttlMinutes : null,
    };

    const response = await fetch(url.toString(), {
        method: 'POST',
        headers,
        body: JSON.stringify(body),
    });

    const text = await response.text();
    console.log(`HTTP ${response.status} ${response.statusText}`);

    try {
        const json = text ? (JSON.parse(text) as unknown) : null;
        console.dir(json, { depth: null });
    } catch {
        console.log(text);
    }

    if (!response.ok) {
        process.exitCode = 1;
    }
}

void main();
