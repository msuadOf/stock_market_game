import { appendFile } from "node:fs/promises";
import { once } from "node:events";
import { spawn } from "node:child_process";

export const PARITY_SETUP = {
  stocks: [{ code: "600101", exchange: "Shanghai", category: "MainBoard", initial_price: 1_000, limit_pct: 0.1, tick: 1, total_shares: "10000000", float_shares: 100_000 }],
  npcs: { retail_count: 0, inst_count: 1, hot_count: 0, retail_cash_median: 10_000_000 },
  config: { commission_rate: 0.00025, commission_min: 500, stamp_tax_rate: 0.0005, default_limit: 0.1, st_limit: 0.1, lot_size: 100, starting_cash: 1_000_000_000 },
  strategy_params: { retail: { arrival_rate: 0.5, order_size_mean: 100, chase_prob: 0.2, tick_cents: 1 }, inst: { margin: 0.05, order_size: 200 }, hot: { lookback: 3, trend_threshold: 0.02, order_size: 200 } },
  ticks_per_day: 30,
  auction_ticks: 0,
  closing_auction_ticks: 0,
  history_len: 5,
  t1_enabled: true,
  float_allocation: "Random",
  start_date: "2030-01-01",
  simulation_policy_id: "a-share-simulation-v1",
};

export const CLOSED_DAY_PARITY_SETUP = {
  ...PARITY_SETUP,
  start_date: "2030-04-19",
};

export const NORMAL_DAY_PARITY_SETUP = {
  ...PARITY_SETUP,
  start_date: "2030-01-02",
};

export function publicSnapshot(snapshot) {
  return {
    seq: snapshot.seq,
    tick: snapshot.tick,
    day: snapshot.day,
    phase: snapshot.phase,
    markets: snapshot.markets,
    accounts: Object.fromEntries(Object.entries(snapshot.accounts).filter(([id]) => id === "0")),
    daily_candles: snapshot.daily_candles,
    active_daily_candles: snapshot.active_daily_candles,
  };
}

export function launch(command, args, output, env = {}) {
  const child = spawn(command, args, { detached: true, env: { ...process.env, ...env }, stdio: ["ignore", "pipe", "pipe"] });
  child.stdout.on("data", (chunk) => void appendFile(output, chunk));
  child.stderr.on("data", (chunk) => void appendFile(output, chunk));
  return child;
}

export async function stop(child, graceMs = 5_000) {
  if (child.exitCode !== null || child.signalCode !== null) {
    return { pid: child.pid, exited: true, forced: false, exitCode: child.exitCode, signalCode: child.signalCode };
  }
  let forced = false;
  try {
    process.kill(-child.pid, "SIGTERM");
  } catch (error) {
    if (!(error && typeof error === "object" && error.code === "ESRCH")) throw error;
    return { pid: child.pid, exited: true, forced: false, exitCode: child.exitCode, signalCode: child.signalCode };
  }
  await Promise.race([once(child, "exit"), new Promise((resolveDelay) => setTimeout(resolveDelay, graceMs))]);
  if (child.exitCode === null) {
    forced = true;
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch (error) {
      if (!(error && typeof error === "object" && error.code === "ESRCH")) throw error;
    }
    if (child.exitCode === null && child.signalCode === null) await once(child, "exit");
  }
  return { pid: child.pid, exited: child.exitCode !== null || child.signalCode !== null, forced, exitCode: child.exitCode, signalCode: child.signalCode };
}

export async function waitFor(url, attempts = 100) {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    try {
      const response = await fetch(url, { signal: AbortSignal.timeout(1_000) });
      if (response.ok) return;
    } catch (error) {
      if (!(error instanceof TypeError)) throw error;
    }
    await new Promise((resolveDelay) => setTimeout(resolveDelay, 100));
  }
  throw new Error(`timed out waiting for ${url}`);
}

export async function jsonResponse(response) {
  const text = await response.text();
  if (!response.ok) throw new Error(`HTTP ${response.status}: ${text}`);
  return text ? JSON.parse(text) : null;
}
