import type { AccountSnap, Cents } from "../types/engine.ts";
import type { RootState } from "../store/store.ts";
import { selectPlayerAccount } from "../store/remote-membership.ts";

export interface PortfolioInput {
  account: AccountSnap | null;
  heldPrices: Readonly<Record<string, Cents>>;
}

export function selectPortfolioInput(state: RootState): PortfolioInput {
  const account = selectPlayerAccount(state);
  const markets = state.snapshot.snapshot?.markets ?? {};
  const heldPrices = Object.fromEntries(
    Object.entries(account?.positions ?? {})
      .filter(([, position]) => position.qty > 0)
      .map(([code]) => {
        if (!Object.hasOwn(markets, code)) throw new Error(`持仓 ${code} 缺少行情，不能估值`);
        return [code, markets[code].last_price];
      }),
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
