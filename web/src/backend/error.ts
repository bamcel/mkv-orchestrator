export type ApiErrorOptions = {
  code?: string;
  status?: number;
  details?: unknown;
  cause?: unknown;
};

/** A consistent error shape for both HTTP failures and Tauri invoke failures. */
export class ApiError extends Error {
  readonly code: string;
  readonly status?: number;
  readonly details?: unknown;

  constructor(message: string, options: ApiErrorOptions = {}) {
    super(message, { cause: options.cause });
    this.name = "ApiError";
    this.code = options.code ?? "UNKNOWN";
    this.status = options.status;
    this.details = options.details;
  }
}

type ErrorRecord = Record<string, unknown>;

function isRecord(value: unknown): value is ErrorRecord {
  return typeof value === "object" && value !== null;
}

function nonEmptyString(value: unknown): string | undefined {
  return typeof value === "string" && value.trim().length > 0 ? value : undefined;
}

export function normalizeApiError(error: unknown, fallbackMessage = "The MKVO request failed."): ApiError {
  if (error instanceof ApiError) {
    return error;
  }

  if (error instanceof Error) {
    return new ApiError(error.message || fallbackMessage, {
      code: nonEmptyString((error as Error & { code?: unknown }).code) ?? "TRANSPORT_ERROR",
      cause: error
    });
  }

  if (isRecord(error)) {
    const nestedError = isRecord(error.error) ? error.error : undefined;
    const message =
      nonEmptyString(error.message) ??
      nonEmptyString(nestedError?.message) ??
      nonEmptyString(error.error) ??
      fallbackMessage;
    const status = typeof error.status === "number" ? error.status : undefined;
    const code = nonEmptyString(error.code) ?? nonEmptyString(nestedError?.code) ?? "TRANSPORT_ERROR";
    return new ApiError(message, {
      code,
      status,
      details: error.details ?? nestedError?.details ?? error,
      cause: error
    });
  }

  if (typeof error === "string") {
    try {
      const parsed = JSON.parse(error) as unknown;
      if (parsed !== error) {
        return normalizeApiError(parsed, fallbackMessage);
      }
    } catch {
      // Plain command errors are expected to be strings during early migration.
    }
  }

  return new ApiError(nonEmptyString(error) ?? fallbackMessage, {
    code: "TRANSPORT_ERROR",
    cause: error
  });
}
