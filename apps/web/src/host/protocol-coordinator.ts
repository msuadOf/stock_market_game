import {
  createProtocolState,
  ProtocolError,
  reduceEngineUpdate,
  type ProtocolReduction,
  type ProtocolState,
} from "./protocol/index.ts";
import type { HostFailure, HostUpdate } from "./host-update.ts";

export type ProtocolCoordinatorStatus =
  | { readonly kind: "idle" }
  | { readonly kind: "ready"; readonly state: ProtocolState }
  | { readonly kind: "failure"; readonly failure: HostFailure; readonly state: ProtocolState | null };

export interface ProtocolCoordinatorCallbacks {
  readonly onBaseline: (state: ProtocolState, update: Extract<HostUpdate, { type: "baseline" }>) => void;
  readonly onApplied: (reduction: Extract<ProtocolReduction, { kind: "applied" }>, metadata: { readonly civilDate: string | null; readonly revision: string | null }) => void;
  readonly onFailure: (failure: HostFailure) => void;
}

export class ProtocolCoordinator {
  private current: ProtocolState | null = null;
  private currentStatus: ProtocolCoordinatorStatus = { kind: "idle" };
  private readonly callbacks: ProtocolCoordinatorCallbacks;

  constructor(callbacks: ProtocolCoordinatorCallbacks) {
    this.callbacks = callbacks;
  }

  status(): ProtocolCoordinatorStatus {
    return this.currentStatus;
  }

  accept(update: HostUpdate): void {
    switch (update.type) {
      case "baseline":
        this.installBaseline(update);
        return;
      case "protocol":
        this.applyProtocol(update);
        return;
      default:
        return assertNever(update);
    }
  }

  fail(failure: HostFailure): void {
    this.currentStatus = { kind: "failure", failure, state: this.current };
    this.callbacks.onFailure(failure);
  }

  private installBaseline(update: Extract<HostUpdate, { type: "baseline" }>): void {
    const state = createProtocolState(update.snapshot, update.generation);
    const hydrated: ProtocolState = {
      ...state,
      securities: update.securities,
      publicPublicationIds: update.publicPublicationIds,
    };
    this.current = hydrated;
    this.currentStatus = { kind: "ready", state: hydrated };
    this.callbacks.onBaseline(hydrated, update);
  }

  private applyProtocol(update: Extract<HostUpdate, { type: "protocol" }>): void {
    if (this.current === null) {
      this.fail({
        code: "PROTOCOL_BASELINE_REQUIRED",
        where: "protocol-coordinator",
        message: "收到协议更新前必须先安装权威基线",
      });
      return;
    }
    try {
      const reduction = reduceEngineUpdate(this.current, update.generation, update.update);
      this.current = reduction.state;
      this.currentStatus = { kind: "ready", state: reduction.state };
      if (reduction.kind === "applied") {
        this.callbacks.onApplied(reduction, { civilDate: update.civilDate, revision: update.revision });
      }
    } catch (error) {
      if (error instanceof ProtocolError) {
        this.fail({ code: error.code, where: error.where, message: error.message });
        return;
      }
      this.fail({
        code: "PROTOCOL_UNEXPECTED",
        where: "protocol-coordinator",
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }
}

function assertNever(value: never): never {
  throw new Error(`未处理宿主更新：${String(value)}`);
}
