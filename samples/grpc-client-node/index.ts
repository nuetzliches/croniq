/* Minimal Node.js gRPC client for Croniq */
import path from "node:path";
import * as grpc from "@grpc/grpc-js";
import * as protoLoader from "@grpc/proto-loader";

const PROTO_PATH = path.join(
  __dirname,
  "..",
  "..",
  "src",
  "Croniq.Rpc.Client",
  "Protos",
  "scheduler.proto"
);

const packageDefinition = protoLoader.loadSync(PROTO_PATH, {
  keepCase: true,
  longs: String,
  enums: String,
  defaults: false,
  oneofs: true,
});

type UpsertScheduleRequest = {
  job_key: string;
  cron_expression: string;
  description?: string;
};

type UpsertScheduleResponse = {
  trigger_id: string;
  job_key: string;
};

type TriggerJobRequest = {
  job_key: string;
};

type TriggerJobResponse = {
  status: string;
};

type SchedulerClient = grpc.Client & {
  upsertSchedule: (
    req: UpsertScheduleRequest,
    md: grpc.Metadata,
    cb: (err: grpc.ServiceError | null, res?: UpsertScheduleResponse) => void
  ) => void;
  triggerJob: (
    req: TriggerJobRequest,
    md: grpc.Metadata,
    cb: (err: grpc.ServiceError | null, res?: TriggerJobResponse) => void
  ) => void;
};

const schedulerPackage = grpc.loadPackageDefinition(packageDefinition) as any;

const endpoint = process.env.CRONIQ_ENDPOINT || "localhost:5080";
const apiKey = process.env.CRONIQ_API_KEY || "dev-key";
const tenantId = process.env.CRONIQ_TENANT_ID || "1";
const environmentTag = process.env.CRONIQ_ENVIRONMENT || "dev";
const jobKey =
  process.env.CRONIQ_JOB_KEY ||
  `${tenantId}:${environmentTag}:ops:node-demo`;

const client = new schedulerPackage.croniq.rpc.Scheduler(
  endpoint,
  grpc.credentials.createInsecure()
) as SchedulerClient;

const upsertSchedule = (req: UpsertScheduleRequest, md: grpc.Metadata) =>
  new Promise<UpsertScheduleResponse>((resolve, reject) => {
    client.upsertSchedule(req, md, (err, res) => {
      if (err) {
        reject(err);
        return;
      }
      resolve(res as UpsertScheduleResponse);
    });
  });

const triggerJob = (req: TriggerJobRequest, md: grpc.Metadata) =>
  new Promise<TriggerJobResponse>((resolve, reject) => {
    client.triggerJob(req, md, (err, res) => {
      if (err) {
        reject(err);
        return;
      }
      resolve(res as TriggerJobResponse);
    });
  });

const metadata = new grpc.Metadata();
metadata.set("x-croniq-key", apiKey);

async function main() {
  console.log(
    `Croniq gRPC demo -> ${endpoint} (tenant ${tenantId}/${environmentTag})`
  );

  try {
    const upsert = await upsertSchedule(
      {
        job_key: jobKey,
        cron_expression: "0/5 * * * * ?",
        description: "node grpc demo",
      },
      metadata
    );

    console.log(`Upserted trigger=${upsert.trigger_id} job=${upsert.job_key}`);

    const trigger = await triggerJob({ job_key: jobKey }, metadata);
    console.log(`Trigger status=${trigger.status}`);
  } catch (err) {
    const serviceError = err as grpc.ServiceError | undefined;
    if (serviceError && typeof serviceError.code !== "undefined") {
      console.error(
        `gRPC error: ${serviceError.code} - ${serviceError.details || serviceError.message
        }`
      );
    } else {
      console.error(err);
    }
  }
}

main();
