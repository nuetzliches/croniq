import * as os from "node:os";
import { WorkerClient } from "./index";

const baseUrl = process.env.CRONIQ_API_BASEURL || "http://localhost:5080";
const tenantId = process.env.CRONIQ_TENANT_ID || "default";
const environment = process.env.CRONIQ_ENVIRONMENT || "dev";
const apiKey = process.env.CRONIQ_API_KEY || "";
const runnerId =
  process.env.CRONIQ_RUNNER_ID || `node-${os.hostname()}-${process.pid}`;

const client = new WorkerClient({
  baseUrl,
  tenantId,
  environment,
  apiKey,
});

console.log("Croniq HTTP worker (node)");
console.log(`- base_url:    ${baseUrl}`);
console.log(`- tenant_id:   ${tenantId}`);
console.log(`- environment: ${environment}`);
console.log(`- runner_id:   ${runnerId}`);

async function loop() {
  while (true) {
    const leases = await client.poll({
      runnerId,
      batchSize: 1,
      waitForMs: 25000,
    });

    for (const lease of leases) {
      console.log(
        `claimed lease: jobKey=${lease.jobKey} triggerId=${lease.triggerId} leaseId=${lease.leaseId}`
      );
      await client.ack({ runnerId, lease, succeeded: true });
      console.log(`acked lease: leaseId=${lease.leaseId}`);
    }
  }
}

loop().catch((err) => {
  console.error(err);
  process.exit(1);
});
