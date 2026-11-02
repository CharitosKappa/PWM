/** A failure the popup can show: `code` is stable (core codes, D31, or the background's own). */
export class ServiceError extends Error {
  constructor(readonly code: string) {
    super(code);
  }
}
