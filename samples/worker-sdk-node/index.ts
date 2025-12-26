export type Lease = {
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

export type WorkerClientConfig = {
  baseUrl: string;
  tenantId: string;
  environment?: string;
  apiKey?: string;
  bearerToken?: string;
  fetchImpl?: typeof fetch;
};

export class CroniqError extends Error {
  status: number;
  body: string;

  constructor(message: string, status: number, body: string) {
    super(message);
    this.name = "CroniqError";
    this.status = status;
    this.body = body;
  }
}

export class LeaseConflictError extends CroniqError {
  constructor(body: string) {
    super("lease conflict", 409, body);
    this.name = "LeaseConflictError";
  }
}

export class LeaseNotFoundError extends CroniqError {
  constructor(body: string) {
    super("lease not found", 404, body);
    this.name = "LeaseNotFoundError";
  }
}

type PostResult<T> = {
  status: number;
  json: T | null;
};

export class WorkerClient {
  private baseUrl: string;
  private tenantId: string;
  private environment: string;
  private apiKey: string;
  private bearerToken: string;
  private fetchImpl: typeof fetch;

  constructor({
    baseUrl,
    tenantId,
    environment,
    apiKey,
    bearerToken,
    fetchImpl,
  }: WorkerClientConfig) {
    if (!baseUrl) {
      throw new Error("baseUrl is required");
    }
    if (!tenantId) {
      throw new Error("tenantId is required");
    }
    if (!apiKey && !bearerToken) {
      throw new Error("apiKey or bearerToken is required");
    }

    const resolvedFetch = fetchImpl ?? globalThis.fetch;
    if (!resolvedFetch) {
      throw new Error("fetch is not available; provide fetchImpl");
    }

    this.baseUrl = baseUrl.replace(/\/+$/, "");
    this.tenantId = tenantId;
    this.environment = environment || "";
    this.apiKey = apiKey || "";
    this.bearerToken = bearerToken || "";
    this.fetchImpl = resolvedFetch.bind(globalThis);
  }

  async poll({ runnerId, batchSize = 1, waitForMs = 0 }: PollRequest): Promise<Lease[]> {
    const body = { runnerId, batchSize, waitForMs };
    const result = await this.postJson<{ leases?: Lease[] }>(`/work/poll`, body);
    return (result.json && result.json.leases) || [];
  }

  async renew({ runnerId, lease }: RenewRequest): Promise<{ renewed: boolean; lease: Lease | null }> {
    try {
      const result = await this.postJson<{ renewed?: boolean; lease?: Lease }>(`/work/renew`, {
        runnerId,
        lease,
      });
      return {
        renewed: result.json ? !!result.json.renewed : false,
        lease: result.json && result.json.lease ? result.json.lease : null,
      };
    } catch (err) {
      if (err instanceof LeaseNotFoundError) {
        return { renewed: false, lease: null };
      }
      throw err;
    }
  }

  async ack({
    runnerId,
    lease,
    succeeded,
    nextFireTimeUtc,
    deadLetterReason,
  }: AckRequest): Promise<void> {
    const body: AckRequest = { runnerId, lease, succeeded };
    if (nextFireTimeUtc) {
      body.nextFireTimeUtc = nextFireTimeUtc;
    }
    if (deadLetterReason) {
      body.deadLetterReason = deadLetterReason;
    }
    await this.postJson(`/work/ack`, body);
  }

  private buildUrl(path: string): string {
    const url = new URL(
      `${this.baseUrl}/tenants/${encodeURIComponent(this.tenantId)}${path}`
    );
    if (this.environment) {
      url.searchParams.set("environment", this.environment);
    }
    return url.toString();
  }

  private headers(): Record<string, string> {
    const headers: Record<string, string> = { "Content-Type": "application/json" };
    if (this.bearerToken) {
      headers.Authorization = `Bearer ${this.bearerToken}`;
      return headers;
    }
    headers["X-Croniq-Key"] = this.apiKey;
    return headers;
  }

  private async postJson<T>(path: string, body: unknown): Promise<PostResult<T>> {
    const response = await this.fetchImpl(this.buildUrl(path), {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify(body),
    });

    if (response.status === 204) {
      return { status: 204, json: null };
    }

    const text = await response.text();
    const json = text ? tryParseJson(text) : null;

    if (response.status === 404) {
      throw new LeaseNotFoundError(text);
    }
    if (response.status === 409) {
      throw new LeaseConflictError(text);
    }
    if (!response.ok) {
      throw new CroniqError(`request failed (${response.status})`, response.status, text);
    }

    return { status: response.status, json: json as T };
  }
}

function tryParseJson(value: string): unknown {
  try {
    return JSON.parse(value);
  } catch {
    return null;
  }
}
