/**
 * Structured error for dp-rest responses.
 */
export class DpRestError extends Error {
  readonly status: number;
  readonly code: string;
  readonly body: Record<string, unknown> | undefined;

  constructor(status: number, code: string, message: string, body?: Record<string, unknown>) {
    super(message);
    this.name = "DpRestError";
    this.status = status;
    this.code = code;
    this.body = body;
  }

  static async fromResponse(res: Response): Promise<DpRestError> {
    let body: Record<string, unknown> | undefined;
    try {
      const j = (await res.clone().json()) as Record<string, unknown>;
      if (j && typeof j === "object") body = j;
    } catch {
      // not JSON — fall through.
    }
    const code = (body?.["code"] as string | undefined) ?? `http_${res.status}`;
    const message = (body?.["error"] as string | undefined)
      ?? (body?.["message"] as string | undefined)
      ?? `HTTP ${res.status}`;
    return new DpRestError(res.status, code, message, body);
  }
}

/** Type guard — narrows an unknown thrown value to a `DpRestError`. */
export function isDpRestError(e: unknown): e is DpRestError {
  return e instanceof DpRestError;
}
