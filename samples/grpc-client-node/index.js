/* Minimal Node.js gRPC client for Croniq */
const path = require('path');
const util = require('util');
const grpc = require('@grpc/grpc-js');
const protoLoader = require('@grpc/proto-loader');

const PROTO_PATH = path.join(__dirname, '..', '..', 'src', 'Croniq.Rpc.Client', 'Protos', 'scheduler.proto');

const packageDefinition = protoLoader.loadSync(PROTO_PATH, {
  keepCase: true,
  longs: String,
  enums: String,
  defaults: false,
  oneofs: true,
});

const schedulerPackage = grpc.loadPackageDefinition(packageDefinition).croniq.rpc;

const endpoint = process.env.CRONIQ_ENDPOINT || 'localhost:5000';
const apiKey = process.env.CRONIQ_API_KEY || 'dev-key';
const tenantId = process.env.CRONIQ_TENANT_ID || '1';
const environmentTag = process.env.CRONIQ_ENVIRONMENT || 'dev';
const jobKey = process.env.CRONIQ_JOB_KEY || `${tenantId}:${environmentTag}:ops:node-demo`;

const client = new schedulerPackage.Scheduler(endpoint, grpc.credentials.createInsecure());
const upsertSchedule = util.promisify((req, md, cb) => client.upsertSchedule(req, md, cb));
const triggerJob = util.promisify((req, md, cb) => client.triggerJob(req, md, cb));

const metadata = new grpc.Metadata();
metadata.set('x-croniq-key', apiKey);

async function main() {
  console.log(`Croniq gRPC demo -> ${endpoint} (tenant ${tenantId}/${environmentTag})`);

  try {
    const upsert = await upsertSchedule(
      {
        job_key: jobKey,
        cron_expression: '0/5 * * * * ?',
        description: 'node grpc demo',
      },
      metadata,
    );

    console.log(`Upserted trigger=${upsert.trigger_id} job=${upsert.job_key}`);

    const trigger = await triggerJob({ job_key: jobKey }, metadata);
    console.log(`Trigger status=${trigger.status}`);
  } catch (err) {
    if (err && typeof err.code !== 'undefined') {
      console.error(`gRPC error: ${err.code} - ${err.details || err.message}`);
    } else {
      console.error(err);
    }
  }
}

main();
