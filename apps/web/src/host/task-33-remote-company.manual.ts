import type { Snapshot } from "../types/engine.ts";
import { CompanyQueryCoordinator } from "./company-query-coordinator.ts";
import { companyReducer, type CompanyState } from "../store/company-slice.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import { createRemoteHost } from "./remote-host.ts";

const snapshot = {
  seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {},
} satisfies Snapshot;
class FixtureWebSocket {
  static readonly OPEN = 1;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onopen: ((event: Event) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;
  readyState = FixtureWebSocket.OPEN;
  send(_value: string): void {}
  close(): void {}
}

const socket = new FixtureWebSocket();
const host = await createRemoteHost(DEFAULT_SETUP, 1n, {
  baseUrl: "http://remote-fixture.test",
  fetchFn: async (input) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return new Response(JSON.stringify({ session_id: "task-33" }));
    if (url.includes("/api/snapshot")) return new Response(JSON.stringify(snapshot));
    return new Response(null, { status: 200 });
  },
  webSocketFactory: () => socket as unknown as WebSocket,
});
let state: CompanyState | undefined;
const coordinator = new CompanyQueryCoordinator(host, (action) => { state = companyReducer(state, action); });
host.start((update) => {
  if (update.type === "baseline") {
    coordinator.installBaseline({ civilDate: update.civilDate, revision: update.revision, seq: update.snapshot.seq });
  } else {
    coordinator.acceptEvents(update.events, { fromSeq: update.fromSeq, toSeq: update.toSeq }, {
      civilDate: update.civilDate, revision: update.revision,
    });
  }
});
socket.onmessage?.({ data: JSON.stringify({ Baseline: {
  snapshot, civil_date: "2030-01-02", public_revision: 7, timeline_generation: 1, public_report_ids: [],
} }) } as MessageEvent);
await new Promise<void>((resolve) => setImmediate(resolve));
socket.onmessage?.({ data: JSON.stringify({ PublisherFrame: {
  from_seq: 1, to_seq: 1, events: [{ CivilDateAdvanced: {
    seq: 1, settled_date: "2030-01-02", next_date: "2030-01-03", next_status: "Trading",
  } }], runtime_snapshot: { ...snapshot, seq: 1, tick: 1 }, civil_date: "2030-01-03", public_revision: 8,
} }) } as MessageEvent);
await new Promise<void>((resolve) => setImmediate(resolve));
socket.onmessage?.({ data: JSON.stringify({ PriceTick: { seq: 2 } }) } as MessageEvent);
await new Promise<void>((resolve) => setImmediate(resolve));
host.dispose();
console.log(JSON.stringify({
  baselineCivilDate: "2030-01-02",
  baselineRevision: "7",
  stateCivilDate: state?.civilDate,
  stateRevision: state?.revision,
  stateLastEventSeq: state?.lastEventSeq,
  standaloneEventCivilDate: state?.civilDate,
  standaloneEventRevision: state?.revision,
}, null, 2));
