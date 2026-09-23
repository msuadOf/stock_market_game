import type { AccountSnap, Cents } from "../types/engine.ts";
import type { RootState } from "../store/store.ts";

export interface PortfolioInput {
  account: AccountSnap | null;
  heldPrices: Readonly<Record<string, Cents>>;
}

export function selectPortfolioInput(state: RootState): PortfolioInput {
  const account = state.snapshot.snapshot?.accounts["0"] ?? null;
  const markets = state.snapshot.snapshot?.markets ?? {};
  const heldPrices = Object.fromEntries(
    Object.entries(account?.positions ?? {})
      .filter(([, position]) => position.qty > 0)
      .map(([code]) => [code, markets[code]?.last_price ?? 0]),
  );
  return { account, heldPrices };
}

export function portfolioInputEqual(left: PortfolioInput, right: PortfolioInput): boolean {
  if (left.account !== right.account) return false;
  const leftCodes = Object.keys(left.heldPrices);
  const rightCodes = Object.keys(right.heldPrices);
  return leftCodes.length === rightCodes.length
    && leftCodes.every((code) => left.heldPrices[code] === right.heldPrices[code]);
}
