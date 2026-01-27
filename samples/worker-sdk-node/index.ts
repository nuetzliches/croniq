export type Lease = {
  executionId: string;
  leaseId: string;
  triggerId: string;
  jobKey: string;
  fireAtUtc: string;
  leaseExpiresAtUtc: string;
  payload?: string | null;
};

export type PollRequest = {
  runnerId: string;
  batchSize?: number;
  waitForMs?: number;
};

export type RenewRequest = {
  runnerId: string;
  lease: Lease;
};

export type AckRequest = {
  runnerId: string;
  lease: Lease;
  succeeded: boolean;
  nextFireTimeUtc?: string;
  deadLetterReason?: string;
};

export type WorkEvent = {
  message: string;
  level?: string;
  timestampUtc?: string;
  properties?: Record<string, string>;
  eventType?: string;
};

throw new Error('Deprecated: moved to sdk/runner-node.');
runnerId: string;
