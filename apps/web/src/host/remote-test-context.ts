import { DEFAULT_SETUP } from "../config/defaults.ts";

export function remoteTestContext(sessionId = "session-1", generation = "1", seed = "1") {
  return {
    session_id: sessionId, setup: DEFAULT_SETUP, seed, resumed: false, generation,
    member: { account_id: "1", admission_funding: { external_cash: "1000000000000" } },
    can_control: true, needs_rejoin: false,
  };
}
