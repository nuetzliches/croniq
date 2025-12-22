import { HttpErrorResponse } from '@angular/common/http';

export type AuthFailureKind = 'unauthenticated' | 'forbidden';

export interface AuthFailure {
  kind: AuthFailureKind;
  status: 401 | 403;
  message: string;
}

export interface AuthFailureMessages {
  unauthenticated?: string;
  forbidden?: string;
}

type ProblemDetails = {
  title?: unknown;
  detail?: unknown;
};

function tryGetProblemDetails(error: HttpErrorResponse): ProblemDetails | null {
  const body = error.error;
  if (!body || typeof body !== 'object') {
    return null;
  }
  return body as ProblemDetails;
}

function tryGetString(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value.trim() : null;
}

export function authFailureFromError(error: unknown, messages: AuthFailureMessages = {}): AuthFailure | null {
  if (!(error instanceof HttpErrorResponse)) {
    return null;
  }

  if (error.status === 401) {
    return {
      kind: 'unauthenticated',
      status: 401,
      message:
        messages.unauthenticated ??
        'Not authenticated (401) — please login via /auth/login.',
    };
  }

  if (error.status === 403) {
    const problem = tryGetProblemDetails(error);
    const title = tryGetString(problem?.title);
    const detail = tryGetString(problem?.detail);

    if (title && title !== 'insufficient-scope') {
      // Prefer API ProblemDetails for mismatch cases; caller-provided messages tend to be too generic.
      const message = detail
        ? `Forbidden (403) — ${detail}`
        : 'Forbidden (403) — your token is not valid for this tenant/environment.';
      return { kind: 'forbidden', status: 403, message };
    }

    return {
      kind: 'forbidden',
      status: 403,
      message:
        messages.forbidden ??
        (detail ? `Forbidden (403) — ${detail}` : 'Forbidden (403) — your token is missing permissions.'),
    };
  }

  return null;
}
