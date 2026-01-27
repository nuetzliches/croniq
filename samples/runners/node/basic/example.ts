import { RunnerClient } from '@croniq/runner-sdk';

const baseUrl = process.env.CRONIQ_API_BASEURL || 'http://localhost:5080';
const tenantId = process.env.CRONIQ_TENANT_ID || 'default';
const environment = process.env.CRONIQ_ENVIRONMENT || 'dev';
const apiKey = process.env.CRONIQ_API_KEY || '';
const runnerId = process.env.CRONIQ_RUNNER_ID || 'default';

const client = new RunnerClient({
    baseUrl,
    tenantId,
    environment,
    apiKey,
});

console.log('Croniq HTTP runner (node)');
console.log(`- base_url:    ${baseUrl}`);
console.log(`- tenant_id:   ${tenantId}`);
console.log(`- environment: ${environment}`);
console.log(`- runner_id:   ${runnerId}`);

async function heartbeat() {
    try {
        await client.heartbeat({
            runnerId,
            environmentTag: environment,
            seenAtUtc: new Date().toISOString(),
        });
    } catch (err) {
        console.warn('heartbeat failed', err);
    }
}

void heartbeat();
setInterval(() => {
    void heartbeat();
}, 15000);

async function loop() {
    while (true) {
        const leases = await client.poll({
            runnerId,
            batchSize: 1,
            waitForMs: 25000,
        });

        for (const lease of leases) {
            console.log(
                `claimed lease: jobKey=${lease.jobKey} triggerId=${lease.triggerId} leaseId=${lease.leaseId}`,
            );
            if (lease.executionMode || lease.invocationSource) {
                console.log(
                    `- intent: mode=${lease.executionMode ?? 'normal'} source=${lease.invocationSource ?? 'schedule'}`,
                );
            }
            await client.events({
                runnerId,
                lease,
                events: [
                    {
                        message: `processing execution ${lease.executionId}`,
                        level: 'Information',
                        eventType: 'runner',
                    },
                ],
            });
            await client.ack({ runnerId, lease, succeeded: true });
            console.log(`acked lease: leaseId=${lease.leaseId}`);
        }
    }
}

loop().catch((err) => {
    console.error(err);
    process.exit(1);
});
