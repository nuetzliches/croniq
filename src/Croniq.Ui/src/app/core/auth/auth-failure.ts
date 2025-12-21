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
    return {
      kind: 'forbidden',
      status: 403,
      message: messages.forbidden ?? 'Forbidden (403) — your token is missing permissions.',
    };
  }

  return null;
}
