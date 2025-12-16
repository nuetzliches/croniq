import { mkdir, writeFile } from 'node:fs/promises';
import http from 'node:http';
import https from 'node:https';
import { dirname, resolve } from 'node:path';

import {
    DEFAULT_OPENAPI_ENDPOINT,
    SNAPSHOT_RELATIVE_PATH,
} from '../config/openapi-zod-client.config';

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

async function main(): Promise<void> {
    const openApiUrl = process.env.CRONIQ_OPENAPI_URL?.trim() || DEFAULT_OPENAPI_ENDPOINT;
    const outputPath = resolve(process.cwd(), SNAPSHOT_RELATIVE_PATH);

    const document = await fetchJson(openApiUrl);

    await mkdir(dirname(outputPath), { recursive: true });
    await writeFile(outputPath, JSON.stringify(document, null, 2) + '\n', 'utf8');

    console.log(`✓ Wrote OpenAPI snapshot to ${outputPath}`);
}

main().catch((error) => {
    console.error('Failed to snapshot Swagger/OpenAPI document', error);
    process.exitCode = 1;
});
