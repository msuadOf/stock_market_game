import { parseRemoteMessage } from "./remote-wire.ts";

const message = parseRemoteMessage(JSON.stringify({
  Baseline: {
    timeline_generation: 1,
    snapshot: { seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} },
    civil_date: "2030-01-01",
    public_revision: 0,
    public_report_ids: [],
  },
}));
console.log(message);
