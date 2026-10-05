import type { GameConfig, Intent, MarketSnap, SessionSetup, Snapshot } from "../types/engine.ts";
import { aSharePriceLimits, maxAShareOrderQuantity, parseShareQuantity, parseYuanPrice, validateAShareQuantity } from "../utils/trade-input.ts";
import { moneyToBigInt, moneyFromBigInt, centsToYuanText, yuanTextToCents } from "../utils/money.ts";

export type TradeSide = "Buy" | "Sell";
export type PriceMode = "fixed" | "bid" | "ask" | "lowest" | "highest";
export type BookField = "price" | "quantity";
export interface TradeDraft { priceText: string; quantityText: string; priceMode: PriceMode; orderKind: "limit" | "market"; interval: number }
export interface RepeatingTrade { code: string; side: TradeSide; enabled: boolean; status: string; nextSecond: number }
interface Options { setup(): SessionSetup; snapshot(): Snapshot | null; autoAllowed(): boolean; submit(intent: Intent): Promise<void> }

export function fractionLots(lots: bigint, divisor: number): bigint {
  if (!Number.isSafeInteger(divisor) || divisor < 1 || lots < 0n) throw new Error("数量比例必须使用非负手数和正整数除数");
  return lots / BigInt(divisor);
}
function roundEven(value: bigint, shift: number): bigint {
  if (shift <= 0) return value << BigInt(-shift);
  const divisor = 1n << BigInt(shift), quotient = value / divisor, remainder = value % divisor;
  return quotient + (remainder * 2n > divisor || (remainder * 2n === divisor && quotient % 2n !== 0n) ? 1n : 0n);
}
function binaryFloat(value: bigint, exponent: number): [bigint, number] {
  const shift = Math.max(0, value.toString(2).length - 53);
  return [roundEven(value, shift), exponent + shift];
}
/** 与 engine Money::apply_rate 的 binary64 乘法及银行家舍入一致，金额始终用 BigInt。 */
export function applyFeeRate(gross: bigint, rate: number): bigint {
  if (gross < 0n || !Number.isFinite(rate) || rate < 0) throw new Error("费用计算必须使用非负金额与有限非负费率");
  if (rate === 0 || gross === 0n) return 0n;
  if (rate === 1) return gross;
  const buffer = new ArrayBuffer(8), view = new DataView(buffer);
  view.setFloat64(0, rate); const bits = view.getBigUint64(0);
  const encodedExponent = Number((bits >> 52n) & 2047n);
  const significand = (bits & ((1n << 52n) - 1n)) + (encodedExponent === 0 ? 0n : 1n << 52n);
  const [integer, integerExponent] = binaryFloat(gross, 0);
  const [product, exponent] = binaryFloat(integer * significand, integerExponent + (encodedExponent === 0 ? -1074 : encodedExponent - 1075));
  return roundEven(product, -exponent);
}
export function buyReservation(gross: bigint, config: GameConfig): bigint {
  const commission = applyFeeRate(gross, config.commission_rate), minimum = moneyToBigInt(config.commission_min);
  return gross + (commission > minimum ? commission : minimum) + applyFeeRate(gross, 0.00001);
}
export function buyingCapacity(cash: string, price: string, config: GameConfig): bigint {
  const available = moneyToBigInt(cash), limit = moneyToBigInt(price);
  if (available < 0n || limit <= 0n) throw new Error("可买计算缺少合法可用资金或价格");
  let low = 0n, high = available / limit / 100n;
  while (low < high) { const middle = (low + high + 1n) / 2n; if (buyReservation(middle * 100n * limit, config) <= available) low = middle; else high = middle - 1n; }
  return low;
}
export function pickBookDraft(market: MarketSnap, side: TradeSide, index: number, field: BookField, quantityText: string, capacity: bigint) {
  const levels = side === "Buy" ? market.asks : market.bids;
  const level = levels[index]; if (!level || index < 0 || index >= 5) throw new Error("该盘口档位没有真实挂单");
  const accumulated = levels.slice(0, index + 1).reduce((total, item) => total + BigInt(item[1]), 0n) / 100n;
  return { priceText: centsToYuanText(level[0]), quantityText: field === "price" ? quantityText : String(accumulated < capacity ? accumulated : capacity) };
}
/** 压缩休市后的游戏秒进度；与 TradingTimeline 相同的阶段比例，保留亚秒间隔精度。 */
export function tradingSecond(tick: number, setup: SessionSetup): number {
  const day = Math.floor(tick / setup.ticks_per_day), offset = tick % setup.ticks_per_day;
  const opening = setup.auction_ticks, closing = setup.closing_auction_ticks, continuous = setup.ticks_per_day - opening - closing;
  const openingSeconds = opening > 0 ? 900 : 0, closingSeconds = closing > 0 ? 180 : 0;
  const continuousSeconds = closing > 0 ? 14220 : 14400;
  const base = day * (openingSeconds + continuousSeconds + closingSeconds);
  if (opening > 0 && offset < opening) return base + offset * openingSeconds / opening;
  if (closing > 0 && offset >= setup.ticks_per_day - closing) return base + openingSeconds + continuousSeconds + (offset - setup.ticks_per_day + closing) * closingSeconds / closing;
  return base + openingSeconds + (offset - opening) * continuousSeconds / continuous;
}
export class QuickTrading {
  private readonly drafts = new Map<string, TradeDraft>();
  private readonly repeating = new Map<string, RepeatingTrade>();
  private readonly pending = new Set<string>();
  private readonly listeners = new Set<() => void>();
  private revision = 0;
  private epoch = 0;
  private lastTick = -1;
  private readonly reservations: { tick: number; code: string; side: TradeSide; qty: number; cash: bigint }[] = [];
  readonly subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };
  readonly version = () => this.revision;
  private readonly options: Options;
  private readonly statuses = new Map<string, string>();
  constructor(options: Options) { this.options = options; }
  status(code: string, side: TradeSide) { return this.statuses.get(this.key(code, side)) ?? ""; }
  private changed() { this.revision += 1; this.listeners.forEach(listener => listener()); }
  private key(code: string, side: TradeSide) { return `${code}:${side}`; }
  private snapshot(): Snapshot { const snapshot = this.options.snapshot(); if (!snapshot) throw new Error("游戏行情尚未就绪"); return snapshot; }
  draft(code: string, side: TradeSide): TradeDraft {
    const key = this.key(code, side); let draft = this.drafts.get(key);
    if (!draft) { const market = this.options.snapshot()?.markets[code]; draft = { priceText: market ? centsToYuanText(market.last_price) : "", quantityText: "1", priceMode: "fixed", orderKind: "limit", interval: 1 }; this.drafts.set(key, draft); }
    return draft;
  }
  edit(code: string, side: TradeSide, patch: Partial<TradeDraft>) { this.drafts.set(this.key(code, side), { ...this.draft(code, side), ...patch }); this.changed(); }
  price(code: string, side: TradeSide): string {
    const draft = this.draft(code, side), market = this.snapshot().markets[code];
    if (!market) throw new Error(`缺少 ${code} 的真实行情`);
    if (draft.orderKind === "market" || draft.priceMode === "highest" || draft.priceMode === "lowest") {
      const spec = this.options.setup().stocks.find(stock => stock.code === code); if (!spec) throw new Error(`缺少 ${code} 的交易规则`);
      const limits = aSharePriceLimits(market.last_close, spec.category);
      return draft.priceMode === "lowest" && draft.orderKind !== "market" ? limits.down : limits.up;
    }
    if (draft.priceMode === "bid" || draft.priceMode === "ask") { const price = draft.priceMode === "bid" ? market.best_bid : market.best_ask; if (price === null) throw new Error(draft.priceMode === "bid" ? "买一暂无报价" : "卖一暂无报价"); return price; }
    return parseYuanPrice(draft.priceText);
  }
  capacity(code: string, side: TradeSide): bigint {
    const snapshot = this.snapshot(), account = snapshot.accounts[0]; if (!account) throw new Error("缺少当前玩家账户");
    if (side === "Sell") { const position = account.positions[code]; const pending = this.reservations.filter(item => item.code === code && item.side === "Sell").reduce((sum, item) => sum + item.qty, 0); return BigInt(Math.max(0, (position ? position.qty - position.t1_locked : 0) - (account.reserved_sell_qty[code] ?? 0) - pending)) / 100n; }
    return buyingCapacity(this.availableCash(), this.price(code, side), this.options.setup().config);
  }
  availableCash(): string {
    const account = this.snapshot().accounts[0];
    if (!account) throw new Error("缺少当前玩家账户");
    const pending = this.reservations.reduce((sum, item) => sum + item.cash, 0n);
    const cash = moneyToBigInt(account.cash) - moneyToBigInt(account.reserved_cash) - pending;
    return moneyFromBigInt(cash > 0n ? cash : 0n);
  }
  pick(code: string, side: TradeSide, index: number, field: BookField) {
    const market = this.snapshot().markets[code]; if (!market) throw new Error(`缺少 ${code} 的盘口`);
    const initial = pickBookDraft(market, side, index, "price", this.draft(code, side).quantityText, 0n);
    this.edit(code, side, { priceMode: "fixed", priceText: initial.priceText, orderKind: "limit" });
    this.edit(code, side, pickBookDraft(market, side, index, field, initial.quantityText, field === "price" ? 0n : this.capacity(code, side)));
  }
  jobs(): readonly RepeatingTrade[] { return [...this.repeating.values()]; }
  busy(code: string, side: TradeSide) { return this.pending.has(this.key(code, side)); }
  error(code: string, side: TradeSide): string | null {
    return this.validation(code, side)?.message ?? null;
  }
  validation(code: string, side: TradeSide): { field: "price" | "quantity" | "form"; message: string } | null {
    let field: "price" | "quantity" | "form" = "form";
    try {
      const draft = this.draft(code, side), snapshot = this.snapshot(), account = snapshot.accounts[0];
      const spec = this.options.setup().stocks.find(stock => stock.code === code); if (!spec || !account) throw new Error("缺少当前股票规则或玩家账户");
      field = "quantity";
      const qty = parseShareQuantity(yuanTextToCents(draft.quantityText));
      const position = account.positions[code];
      const pending = this.reservations.filter(item => item.code === code && item.side === "Sell").reduce((sum, item) => sum + item.qty, 0);
      const sellable = Math.max(0, (position ? position.qty - position.t1_locked : 0) - (account.reserved_sell_qty[code] ?? 0) - pending);
      validateAShareQuantity(side, qty, sellable, maxAShareOrderQuantity(spec.category, draft.orderKind === "market"));
      field = "price"; this.price(code, side);
      field = "quantity";
      if (side === "Buy" && BigInt(qty) / 100n > this.capacity(code, side)) throw new Error("可用资金不足（已计入费用及挂单占用）");
      field = "form";
      if (snapshot.phase === "PreOpen") throw new Error("当前为开盘前等待阶段，暂不接受新申报");
      if (draft.orderKind === "market" && snapshot.phase !== "Continuous") throw new Error("集合竞价仅接受限价委托");
      return null;
    } catch (error) { return { field, message: error instanceof Error ? error.message : String(error) }; }
  }
  async submit(code: string, side: TradeSide): Promise<boolean> {
    const key = this.key(code, side); if (this.pending.has(key)) return false;
    const error = this.error(code, side); if (error) { this.setStatus(code, side, `等待：${error}`); return false; }
    const draft = this.draft(code, side), qty = parseShareQuantity(yuanTextToCents(draft.quantityText)), price = this.price(code, side);
    const intent: Intent = draft.orderKind === "market" ? { PlaceMarket: { code, side, qty } } : { PlaceLimit: { code, side, qty, price: draft.priceMode === "highest" ? "Highest" : draft.priceMode === "lowest" ? "Lowest" : { Fixed: price } } };
    const epoch = this.epoch;
    const reservation = { tick: this.snapshot().tick, code, side, qty, cash: side === "Buy" ? buyReservation(moneyToBigInt(price) * BigInt(qty), this.options.setup().config) : 0n };
    this.reservations.push(reservation); this.pending.add(key); this.changed();
    try { await this.options.submit(intent); if (epoch === this.epoch) this.setStatus(code, side, "已提交，等待引擎受理"); return true; }
    catch (error) { const index = this.reservations.indexOf(reservation); if (index >= 0) this.reservations.splice(index, 1); if (epoch === this.epoch) this.setStatus(code, side, `下单失败：${error instanceof Error ? error.message : String(error)}`); return false; }
    finally { if (epoch === this.epoch) { this.pending.delete(key); this.changed(); } }
  }
  private setStatus(code: string, side: TradeSide, status: string) { this.statuses.set(this.key(code, side), status); const job = this.repeating.get(this.key(code, side)); if (job) job.status = status; this.changed(); }
  toggleAuto(code: string, side: TradeSide) {
    const key = this.key(code, side), old = this.repeating.get(key);
    if (old?.enabled) { old.enabled = false; old.status = "自动已停止，已有挂单保留"; }
    else this.repeating.set(key, { code, side, enabled: true, status: "自动已开启，等待游戏时间到期", nextSecond: tradingSecond(this.snapshot().tick, this.options.setup()) + this.draft(code, side).interval });
    this.changed();
  }
  async advance(snapshot: Snapshot) {
    const epoch = this.epoch;
    if (snapshot.tick <= this.lastTick) return;
    this.lastTick = snapshot.tick;
    for (let index = this.reservations.length - 1; index >= 0; index--) if (this.reservations[index]!.tick < snapshot.tick && !this.pending.has(this.key(this.reservations[index]!.code, this.reservations[index]!.side))) this.reservations.splice(index, 1);
    const now = tradingSecond(snapshot.tick, this.options.setup());
    // 同一交付跨多个间隔时仅用最新权威资源执行一次，不补造过去时点的委托。
    for (const job of this.repeating.values()) if (job.enabled && now >= job.nextSecond && !this.busy(job.code, job.side)) {
      if (!this.options.autoAllowed()) return;
      job.nextSecond = now + this.draft(job.code, job.side).interval;
      await this.submit(job.code, job.side);
      if (epoch !== this.epoch) return;
    }
  }
  async cancelAll(orders: readonly { code: string; id: number }[]): Promise<void> {
    const epoch = this.epoch;
    const failures: string[] = [];
    for (const order of orders) {
      if (epoch !== this.epoch) throw new Error("游戏会话已发生变化，剩余撤单请求已停止；请核对当前挂单");
      try { await this.options.submit({ Cancel: { code: order.code, id: order.id } }); } catch (error) { failures.push(`#${order.id}：${error instanceof Error ? error.message : String(error)}`); }
    }
    if (epoch !== this.epoch) throw new Error("游戏会话已发生变化，请核对当前挂单");
    if (failures.length) throw new Error(`部分撤单请求未提交：${failures.join("；")}`);
  }
  reset() { this.epoch += 1; this.drafts.clear(); this.statuses.clear(); this.repeating.clear(); this.pending.clear(); this.reservations.length = 0; this.lastTick = -1; this.changed(); }
}
