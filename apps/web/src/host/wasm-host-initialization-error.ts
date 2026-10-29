import type { HostFailure } from "./host-update.ts";

export class WasmHostInitializationError extends Error {
  readonly failure: HostFailure;

  constructor(failure: HostFailure) {
    super(`${failure.code} @ ${failure.where}: ${failure.message}`, { cause: failure });
    this.name = "WasmHostInitializationError";
    this.failure = failure;
  }
}
