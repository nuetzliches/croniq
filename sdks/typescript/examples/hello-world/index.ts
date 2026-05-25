// A minimal Croniq runner. Run against a local server with:
//
//   cd sdks/typescript && npm install && npm run build
//   cd examples/hello-world && npm install && npm start
//
// Wire to your local stack:
//
//   CRONIQ_SERVER_URL=http://localhost:4000 CRONIQ_API_KEY=… npm start
//
// The handler is called every time the server dispatches a `hello:world` job;
// register one in your Croniqfile or trigger it via /v1/trigger.

import { consoleLogger, createRunner } from '@nuetzliches/croniq-runner';

const runner = createRunner({
  serverUrl: process.env.CRONIQ_SERVER_URL ?? 'http://localhost:4000',
  apiKey: process.env.CRONIQ_API_KEY,
  capabilities: ['demo'],
  tags: ['lang=typescript', 'env=dev'],
  maxInflight: 5,
  // Tee SDK diagnostics to stdout instead of staying at the default `warn`.
  logger: consoleLogger('info', 'croniq'),
});

runner.handle('hello:world', async (ctx) => {
  ctx.logger.info(`Hello from ${ctx.jobKey} (attempt ${ctx.attempt})`);
  await ctx.logWriter.write('info', `running on ${ctx.runnerId}`);
  // Simulate a bit of work.
  await new Promise((resolve) => setTimeout(resolve, 1_000));
});

const controller = new AbortController();
for (const sig of ['SIGTERM', 'SIGINT'] as const) {
  process.on(sig, () => {
    // eslint-disable-next-line no-console
    console.error(`received ${sig}, draining…`);
    controller.abort();
  });
}

await runner.run(controller.signal);
