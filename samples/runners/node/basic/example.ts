import {
    CroniqRunner,
    RunnerIdInUseError,
    RunnerJobRegistrationDeniedError,
    loadRunnerConfigFromEnv,
} from '@croniq/runner-sdk';

const jobKey = process.env.CRONIQ_JOB_KEY?.trim() || 'samples:node-job';

const config = loadRunnerConfigFromEnv(process.env, {
    runnerApiKeyEnv: 'CRONIQ_RUNNER_NODE_API_KEY',
    defaultRunnerId: 'default',
    runnerApiKeyDefaultRunnerId: 'node-default',
});

const runner = new CroniqRunner({
    ...config,
    heartbeatIntervalMs: 15000,
});

console.log('Croniq runner (node)');
console.log(`- base_url:        ${config.baseUrl}`);
console.log(`- grpc_url:        ${config.grpcBaseUrl ?? config.baseUrl}`);
console.log(`- tenant_id:       ${config.tenantId}`);
console.log(`- environment:     ${config.environment}`);
console.log(`- runner_id:       ${config.runnerId}`);
console.log(`- runner_instance:${config.runnerInstanceId ?? '(auto)'}`);
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
const runTask = runner.start().catch((err) => {
    if (shuttingDown) {
        console.warn('runner stopped during shutdown', err);
        return;
    }
    if (err instanceof RunnerIdInUseError) {
        console.error('runnerId already in use; exiting', err);
    } else if (err instanceof RunnerJobRegistrationDeniedError) {
        console.error('job registration denied; exiting', err);
    } else {
        console.error('runner failed to start', err);
    }
    process.exitCode = 1;
});

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
        await runner.stop();
    } finally {
        await runTask;
        if (!process.exitCode) {
            process.exitCode = 0;
        }
    }
};

process.on('SIGTERM', () => void shutdown('SIGTERM'));
process.on('SIGINT', () => void shutdown('SIGINT'));
if (process.platform === 'win32') {
    process.on('SIGBREAK', () => void shutdown('SIGBREAK'));
}

void runTask;

async function doWork(payload: unknown) {
    if (payload) {
        console.log('payload received', payload);
    }
}
