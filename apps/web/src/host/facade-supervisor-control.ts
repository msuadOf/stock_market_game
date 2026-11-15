import { parseFacadeSupervisorControl as parseLibosControl } from "../../../../packages/libos/browser/facade-supervisor-control.ts";
import { parseHostFailure } from "./protocol-failure.ts";

export type { FacadeSupervisorControlMessage } from "../../../../packages/libos/browser/facade-supervisor-control.ts";

export function parseFacadeSupervisorControl(value: unknown) {
  return parseLibosControl(value, parseHostFailure);
}
