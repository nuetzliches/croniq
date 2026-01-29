import { CroniqRunner, RunnerIdInUseError, RunnerJobRegistrationDeniedError } from '@croniq/runner-sdk';

const env = (key: string, fallback: string) => (process.env[key]?.trim() ? process.env[key]!.trim() : fallback);

const baseUrl = env('CRONIQ_API_BASEURL', 'http://localhost:5080');
const grpcBaseUrl = env('CRONIQ_GRPC_BASEURL', baseUrl);
const tenantId = env('CRONIQ_TENANT_ID', 'default');
const environment = env('CRONIQ_ENVIRONMENT', 'dev');
const runnerApiKey = env('CRONIQ_RUNNER_NODE_API_KEY', '');
const apiKey = runnerApiKey || env('CRONIQ_API_KEY', '');
const bearerToken = env('CRONIQ_BEARER_TOKEN', '');
const runnerIdEnv = process.env.CRONIQ_RUNNER_ID?.trim() ?? '';
const runnerId = runnerIdEnv && !(runnerApiKey && runnerIdEnv.toLowerCase() === 'default')
    ? runnerIdEnv
    : (runnerApiKey ? 'node-default' : 'default');
const runnerInstanceId = process.env.CRONIQ_RUNNER_INSTANCE_ID?.trim();
const jobKey = env('CRONIQ_JOB_KEY', 'samples:node-job');

if ((!!apiKey && !!bearerToken) || (!apiKey && !bearerToken)) {
    throw new Error('Set exactly one of CRONIQ_API_KEY or CRONIQ_BEARER_TOKEN');
}

const runner = new CroniqRunner({
    baseUrl,
    grpcBaseUrl,
    tenantId,
    environment,
    apiKey: apiKey || undefined,
    bearerToken: bearerToken || undefined,
    runnerId,
    runnerInstanceId,
    transportMode: (process.env.CRONIQ_TRANSPORT_MODE?.trim().toLowerCase() as 'auto' | 'grpc' | 'polling') || 'auto',
    allowTestExecutions: process.env.CRONIQ_ALLOW_TEST_EXECUTIONS === 'true',
    maxInflight: process.env.CRONIQ_MAX_INFLIGHT ? Number(process.env.CRONIQ_MAX_INFLIGHT) : undefined,
    capabilities: process.env.CRONIQ_CAPABILITIES
        ? process.env.CRONIQ_CAPABILITIES.split(',')
            .map((value) => value.trim())
            .filter(Boolean)
        : undefined,
    heartbeatIntervalMs: 15000,
});

console.log('Croniq runner (node)');
console.log(`- base_url:        ${baseUrl}`);
console.log(`- grpc_url:        ${grpcBaseUrl}`);
console.log(`- tenant_id:       ${tenantId}`);
console.log(`- environment:     ${environment}`);
console.log(`- runner_id:       ${runnerId}`);
console.log(`- runner_instance:${runnerInstanceId ?? '(auto)'}`);
if (jobKey) {
    console.log(`- job_key:         ${jobKey}`);
}

runner.onExecute(
    jobKey,
    async (context, payload, logger) => {
        const startedAt = Date.now();
        logger.info('execution started', {
            executionId: context.executionId,
            jobKey: context.jobKey,
            triggerId: context.triggerId,
            executionMode: context.executionMode,
        });

        await doWork(payload);

        logger.info('execution completed', {
            executionId: context.executionId,
            durationMs: Date.now() - startedAt,
        });
    },
    {
        description: 'Demo job registered by the Node runner sample.',
        metadata: {
            sample: 'node',
            sdk: 'croniq-runner',
        },
    },
);

let shuttingDown = false;
const shutdown = async (signal: string) => {
    if (shuttingDown) {
        return;
    }
    shuttingDown = true;
    console.log(`runner draining due to ${signal}`);
    try {
        await runner.drain(30000);
    } catch (err) {
        console.error('runner drain failed', err);
    } finally {
        process.exit(0);
    }
};

process.on('SIGTERM', () => void shutdown('SIGTERM'));
process.on('SIGINT', () => void shutdown('SIGINT'));
if (process.platform === 'win32') {
    process.on('SIGBREAK', () => void shutdown('SIGBREAK'));
}

runner.start().catch((err) => {
    if (err instanceof RunnerIdInUseError) {
        console.error('runnerId already in use; exiting', err);
    } else if (err instanceof RunnerJobRegistrationDeniedError) {
        console.error('job registration denied; exiting', err);
    } else {
        console.error('runner failed to start', err);
    }
    process.exit(1);
});

async function doWork(payload: unknown) {
    if (payload) {
        console.log('payload received', payload);
    }
}
