import { createProtocolUpdate } from "./host-update.ts";

export function postWorkerProtocol(port: { postMessage(message: unknown): void }, generation: string, update: unknown): void {
  port.postMessage(createProtocolUpdate(generation, update));
}
