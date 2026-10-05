import { DEFAULT_SETUP, AUCTION_VOLUME_LINES_PER_MINUTE, CALL_AUCTION_ENTRY_MINUTES } from "../config/defaults.ts";
import type { SessionSetup } from "../types/engine.ts";

export type TradingTiming = Pick<SessionSetup, "ticks_per_day" | "auction_ticks" | "closing_auction_ticks">;

/** 仅换算展示坐标；交易阶段、撮合及存档中的 tick 仍由 engine 决定。 */
export class TradingTimeline {
  readonly ticksPerDay: number;
  readonly openingTicks: number;
  readonly closingTicks: number;
  readonly auctionEntryTicks: number;
  readonly customized: boolean;
  readonly displayNote: string | null;

  constructor(timing: TradingTiming = DEFAULT_SETUP) {
    const { ticks_per_day: total, auction_ticks: opening, closing_auction_ticks: closing } = timing;
    if (!Number.isSafeInteger(total) || !Number.isSafeInteger(opening) || !Number.isSafeInteger(closing)
      || total <= 0 || opening < 0 || closing < 0 || opening >= total || closing >= total - opening) {
      throw new RangeError("交易日必须包含连续竞价 tick，开盘及收盘窗口不能覆盖整个交易日");
    }
    this.ticksPerDay = total;
    this.openingTicks = opening;
    this.closingTicks = closing;
    this.auctionEntryTicks = opening - Math.floor(opening / 3);
    this.customized = total !== DEFAULT_SETUP.ticks_per_day || opening !== DEFAULT_SETUP.auction_ticks || closing !== DEFAULT_SETUP.closing_auction_ticks;
    const notes = ["自定义交易日时间映射（简化），整数 tick 可使阶段边界粗化"];
    if (opening === 0) notes.push("无开盘集合竞价及 PreOpen");
    else if (opening === this.auctionEntryTicks) notes.push("无独立 PreOpen");
    if (closing === 0) notes.push("无收盘集合竞价");
    this.displayNote = this.customized ? notes.join("；") : null;
    Object.freeze(this);
  }

  eventDay(tick: number): number { return Math.floor(this.eventOffset(tick) / this.ticksPerDay); }

  auctionSlot(tick: number, completed = false): number {
    const offset = this.eventOffset(tick) % this.ticksPerDay;
    if (offset >= this.auctionEntryTicks) throw new RangeError(`竞价 tick ${tick} 超出开盘集合竞价窗口`);
    const count = CALL_AUCTION_ENTRY_MINUTES * AUCTION_VOLUME_LINES_PER_MINUTE;
    return completed ? count - 1 : scale(offset, count, this.auctionEntryTicks);
  }

  minuteSlot(tick: number): number {
    const offset = this.eventOffset(tick) % this.ticksPerDay;
    if (offset < this.openingTicks) throw new RangeError(`连续竞价 tick ${tick} 尚未结束开盘窗口`);
    return Math.floor(this.tradingSecond(offset) / 60);
  }

  clock(tick: number): string {
    if (!Number.isSafeInteger(tick) || tick < 0) throw new RangeError(`游戏 tick 必须是非负安全整数，收到 ${tick}`);
    return this.formatOffset(tick % this.ticksPerDay, false);
  }

  tradeTime(tick: number | undefined): string {
    if (tick === undefined) return "成交时间缺失";
    const offset = this.eventOffset(tick) % this.ticksPerDay + 1;
    return this.formatOffset(offset, true);
  }

  private eventOffset(tick: number): number {
    if (!Number.isSafeInteger(tick) || tick <= 0) throw new RangeError(`行情事件 tick 必须是正安全整数，收到 ${tick}`);
    return tick - 1;
  }

  private tradingSecond(offset: number): number {
    const continuousTicks = this.ticksPerDay - this.openingTicks - this.closingTicks;
    const continuousSeconds = this.closingTicks > 0 ? 237 * 60 : 240 * 60;
    if (this.closingTicks === 0 || offset < this.ticksPerDay - this.closingTicks) {
      return scale(offset - this.openingTicks, continuousSeconds, continuousTicks);
    }
    return continuousSeconds + scale(offset - (this.ticksPerDay - this.closingTicks), 3 * 60, this.closingTicks);
  }

  private formatOffset(offset: number, trade: boolean): string {
    let seconds: number;
    if (offset < this.openingTicks || (trade && offset === this.openingTicks && this.openingTicks > 0)) {
      // 与 engine observation_civil_instant 一致；非三整除时阶段边界由整数 tick 粗化。
      const openingSecond = scale(offset, 15 * 60, this.openingTicks);
      seconds = 9 * 3600 + 15 * 60 + openingSecond;
    } else {
      const continuous = this.tradingSecond(offset);
      const morning = trade ? continuous <= 7200 : continuous < 7200;
      seconds = morning ? 9 * 3600 + 30 * 60 + continuous : 13 * 3600 + continuous - 7200;
    }
    return [Math.floor(seconds / 3600), Math.floor(seconds % 3600 / 60), seconds % 60]
      .map(value => String(value).padStart(2, "0")).join(":");
  }
}

/** 整数比值避免自定义长窗口在阶段边界产生浮点舍入漂移。 */
function scale(offset: number, seconds: number, ticks: number): number {
  if (ticks <= 0) throw new RangeError("不能换算长度为零的交易窗口");
  return Number(BigInt(offset) * BigInt(seconds) / BigInt(ticks));
}

export const DEFAULT_TRADING_TIMELINE = new TradingTimeline();
