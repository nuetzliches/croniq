import { existsSync } from 'node:fs';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';

type RuntimeConfig = {
    apiBaseUrl?: string;
    swaggerUiUrl?: string;
    webhooksAllowUnsignedHooks?: boolean;
};

const CONFIG_RELATIVE_PATH = join('public', 'assets', 'croniq-config.json');
const APIHOST_SETTINGS_RELATIVE_PATHS = [
    join('samples', 'Croniq.Sample.ApiHost', 'appsettings.Development.json'),
    join('samples', 'Croniq.Sample.ApiHost', 'appsettings.json'),
];

async function main(): Promise<void> {
    const configPath = resolve(process.cwd(), CONFIG_RELATIVE_PATH);
    const existing = await loadExistingConfig(configPath);
    const env = await loadEnv();

    const next: RuntimeConfig = {};
    if (existing.config.apiBaseUrl) {
        next.apiBaseUrl = existing.config.apiBaseUrl;
    }
    if (existing.config.swaggerUiUrl) {
        next.swaggerUiUrl = existing.config.swaggerUiUrl;
    }
    if (existing.config.webhooksAllowUnsignedHooks !== undefined) {
        next.webhooksAllowUnsignedHooks = existing.config.webhooksAllowUnsignedHooks;
    }

    const apiBaseUrl = resolveApiBaseUrl(env);
    if (apiBaseUrl) {
        next.apiBaseUrl = apiBaseUrl;
    }

    const swaggerUiUrl = resolveSwaggerUiUrl(env);
    if (swaggerUiUrl) {
        next.swaggerUiUrl = swaggerUiUrl;
    }

    const allowUnsigned = await resolveAllowUnsignedHooks();
    if (allowUnsigned !== undefined) {
        next.webhooksAllowUnsignedHooks = allowUnsigned;
    } else if (next.webhooksAllowUnsignedHooks === undefined) {
        next.webhooksAllowUnsignedHooks = false;
    }

    const serialized = JSON.stringify(next, null, 2) + '\n';
    if (existing.raw === serialized) {
        return;
    }

    await mkdir(dirname(configPath), { recursive: true });
    await writeFile(configPath, serialized, 'utf8');
    console.log(`[Croniq.Ui] runtime config written to ${configPath}`);
}

async function loadExistingConfig(path: string): Promise<{ config: RuntimeConfig; raw: string | null }> {
    if (!existsSync(path)) {
        return { config: {}, raw: null };
    }

    const raw = await readFile(path, 'utf8');
    try {
        const parsed = JSON.parse(raw) as RuntimeConfig;
        return { config: parsed ?? {}, raw };
    } catch (error) {
        console.warn(`[Croniq.Ui] runtime config parse failed; overwriting (${path}).`, error);
        return { config: {}, raw };
    }
}

async function loadEnv(): Promise<Record<string, string>> {
    const fromFile = await loadEnvFromFile();
    const fromProcess = normalizeEnv(process.env);
    return { ...fromFile, ...fromProcess };
}

async function loadEnvFromFile(): Promise<Record<string, string>> {
    const envPath = findEnvFile(process.cwd(), 3);
    if (!envPath) {
        return {};
    }

    try {
        const raw = await readFile(envPath, 'utf8');
        return parseEnvFile(raw);
    } catch (error) {
        console.warn(`[Croniq.Ui] failed to read ${envPath}; skipping.`, error);
        return {};
    }
}

function findEnvFile(startDir: string, maxLevels: number): string | null {
    let current = startDir;
    for (let level = 0; level <= maxLevels; level += 1) {
        const candidate = join(current, '.env');
        if (existsSync(candidate)) {
            return candidate;
        }

        const parent = dirname(current);
        if (parent === current) {
            break;
        }
        current = parent;
    }

    return null;
}

function normalizeEnv(env: Record<string, string | undefined>): Record<string, string> {
    const normalized: Record<string, string> = {};
    for (const [key, value] of Object.entries(env)) {
        if (typeof value === 'string') {
            normalized[key] = value;
        }
    }
    return normalized;
}

function parseEnvFile(contents: string): Record<string, string> {
    const env: Record<string, string> = {};
    const lines = contents.split(/\r?\n/);
    for (const rawLine of lines) {
        const trimmed = rawLine.trim();
        if (!trimmed || trimmed.startsWith('#')) {
            continue;
        }

        const line = trimmed.startsWith('export ') ? trimmed.slice('export '.length) : trimmed;
        const separatorIndex = line.indexOf('=');
        if (separatorIndex < 0) {
            continue;
        }

        const key = line.slice(0, separatorIndex).trim();
        let value = line.slice(separatorIndex + 1).trim();
        if (!key) {
            continue;
        }

        if (
            (value.startsWith('"') && value.endsWith('"')) ||
            (value.startsWith("'") && value.endsWith("'"))
        ) {
            value = value.slice(1, -1);
        }

        env[key] = value;
    }
    return env;
}

function resolveApiBaseUrl(env: Record<string, string>): string | undefined {
    const explicit = pick(env, ['CRONIQ_UI_API_BASEURL']);
    if (explicit) {
        return explicit;
    }

    const port = pick(env, ['CRONIQ_UI_API_PORT']);
    if (!port) {
        return undefined;
    }

    const host = pick(env, ['CRONIQ_UI_API_HOST']) ?? 'localhost';
    const scheme = pick(env, ['CRONIQ_UI_API_SCHEME']) ?? 'http';
    return `${scheme}://${host}:${port}`;
}

function resolveSwaggerUiUrl(env: Record<string, string>): string | undefined {
    return pick(env, ['CRONIQ_UI_SWAGGER_UI_URL', 'CRONIQ_UI_SWAGGER_URL']);
}

async function resolveAllowUnsignedHooks(): Promise<boolean | undefined> {
    const settingsPath = findApiHostSettingsFile(process.cwd(), 3);
    if (!settingsPath) {
        return undefined;
    }

    try {
        const raw = await readFile(settingsPath, 'utf8');
        const parsed = JSON.parse(raw) as {
            Croniq?: {
                Webhooks?: {
                    Security?: {
                        AllowUnsignedHooks?: boolean;
                    };
                };
            };
        };

        const allowUnsigned = parsed?.Croniq?.Webhooks?.Security?.AllowUnsignedHooks;
        return typeof allowUnsigned === 'boolean' ? allowUnsigned : undefined;
    } catch (error) {
        console.warn(`[Croniq.Ui] failed to read ${settingsPath}; skipping.`, error);
        return undefined;
    }
}

function pick(env: Record<string, string>, keys: string[]): string | undefined {
    for (const key of keys) {
        const value = env[key]?.trim();
        if (value) {
            return value;
        }
    }
    return undefined;
}

function findApiHostSettingsFile(startDir: string, maxLevels: number): string | null {
    let current = startDir;
    for (let level = 0; level <= maxLevels; level += 1) {
        for (const relativePath of APIHOST_SETTINGS_RELATIVE_PATHS) {
            const candidate = join(current, relativePath);
            if (existsSync(candidate)) {
                return candidate;
            }
        }

        const parent = dirname(current);
        if (parent === current) {
            break;
        }
        current = parent;
    }

    return null;
}

main().catch((error) => {
    console.error('[Croniq.Ui] failed to generate runtime config.', error);
    process.exitCode = 1;
});
