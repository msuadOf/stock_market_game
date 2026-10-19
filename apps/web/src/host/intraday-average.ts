/**
 * 日内均价（VWAP）与个人成交确认归一层（W3 取回自 18725d07）。
 *
 * VWAP 三件（输入/单点/曲线）为纯计算面，消费 engine-host 既有视图类型
 * （camelCase 输入 + wire snake_case 结果）；个人成交确认消费 B2 新 wire
 * 形态——`stock` 数值 StockId 与 `price_cents`/`gross_cents` 金额字段在
 * 归一层译回视图词表（`code` 六位串 + `price`/`gross`，ADR-0041）。
 */
import type { IntradayAverageCurveInput, IntradayAverageInput, IntradayAverageResult, PersonalTradeConfirmation } from "./engine-host.ts";
import { stockCodeOf } from "./protocol/stock-identity.ts";
import { parseMoney } from "../utils/money.ts";
import { parseTurnoverCents } from "../utils/turnover.ts";

function record(value: unknown, where: string): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error(`${where} 必须是对象`);
  return value as Record<string, unknown>;
}

function decimal(value: unknown, where: string): string {
  if (typeof value !== "string" || value.length > 20 || !/^(0|[1-9]\d*)$/.test(value)) throw new Error(`${where} 必须是规范非负十进制字符串`);
  if (BigInt(value) > 18_446_744_073_709_551_615n) throw new Error(`${where} 超出 u64 范围`);
  return value;
}

function money(value: unknown, where: string): string {
  const parsed = parseMoney(value, where);
  if (BigInt(parsed) < 0n) throw new Error(`${where} 必须是非负分单位金额字符串`);
  return parsed;
}

export function normalizeConfirmationCursor(value: string | null): string | null {
  return value === null ? null : decimal(value, "本人交割单 beforeReceipt");
}

export function normalizeIntradayAverageInput(input: IntradayAverageInput): IntradayAverageInput {
  if (!Number.isSafeInteger(input.tradeCount) || input.tradeCount < 0) throw new Error("VWAP tradeCount 必须是非负安全整数");
  if (!Number.isSafeInteger(input.volumeShares) || input.volumeShares < 0) throw new Error("VWAP volumeShares 必须是非负安全整数");
  return { turnoverCents: parseTurnoverCents(input.turnoverCents, "VWAP turnoverCents"), tradeCount: input.tradeCount, volumeShares: input.volumeShares };
}

export function normalizeIntradayAverageResult(value: unknown): IntradayAverageResult | null {
  if (value === null) return null;
  const source = record(value, "VWAP result");
  const keys = Object.keys(source);
  if (keys.length !== 2 || !Object.hasOwn(source, "turnover_cents") || !Object.hasOwn(source, "volume_shares")) throw new Error("VWAP result 字段不符合契约");
  const volumeShares = source.volume_shares;
  if (!Number.isSafeInteger(volumeShares) || Number(volumeShares) <= 0) throw new Error("VWAP volume_shares 必须是正安全整数");
  const turnoverCents = parseTurnoverCents(source.turnover_cents, "VWAP result.turnover_cents");
  if (turnoverCents === "0") throw new Error("有成交股数时 VWAP 成交额必须为正数");
  return { turnoverCents, volumeShares: Number(volumeShares) };
}

export function normalizeIntradayAverageCurveInput(input: IntradayAverageCurveInput): IntradayAverageCurveInput {
  if (typeof input.seriesKey !== "string" || input.seriesKey.length === 0 || input.seriesKey.length > 256) throw new Error("VWAP seriesKey 必须是 1 至 256 个字符");
  if (!Array.isArray(input.samples) || input.samples.length > 600) throw new Error("VWAP curve 样本数必须在 0 至 600 之间");
  return { seriesKey: input.seriesKey, samples: input.samples.map(normalizeIntradayAverageInput) };
}

export function normalizeIntradayAverageCurveResult(value: unknown, expectedCount: number, samples?: readonly IntradayAverageInput[]): readonly (IntradayAverageResult | null)[] {
  if (!Array.isArray(value) || value.length !== expectedCount) throw new Error(`VWAP curve 结果长度必须为 ${expectedCount}`);
  if (samples !== undefined && samples.length !== expectedCount) throw new Error("VWAP curve 原始事实数量不符请求");
  return value.map((item, index) => {
    const result = normalizeIntradayAverageResult(item);
    if (samples !== undefined) {
      const sample = samples[index];
      if (result === null ? sample.turnoverCents !== "0" || sample.volumeShares !== 0 || sample.tradeCount !== 0 : result.turnoverCents !== sample.turnoverCents || result.volumeShares !== sample.volumeShares) throw new Error("VWAP curve 返回统计与原始成交事实不一致");
    }
    return result;
  });
}

export function normalizePersonalTradeConfirmations(value: unknown): readonly PersonalTradeConfirmation[] {
  if (!Array.isArray(value)) throw new Error("本人交割单必须是数组");
  return value.map((item, index) => {
    const where = `本人交割单[${index}]`;
    const source = record(item, where);
    // B2 wire：stock（数值 StockId）+ price_cents/gross_cents；旧 wire 的
    // code/price/gross 仅是视图词表（ADR-0041——摄入层翻译）。
    const required = ["receipt_id", "civil_date", "stock", "side", "price_cents", "quantity_shares", "gross_cents", "actual_fees"];
    if (Object.keys(source).length !== required.length || required.some((key) => !Object.hasOwn(source, key))) throw new Error(`${where} 字段不符合契约`);
    const fees = record(source.actual_fees, `${where}.actual_fees`);
    if (Object.keys(fees).length !== 3 || !["commission", "stamp_tax", "transfer_fee"].every((key) => Object.hasOwn(fees, key))) throw new Error(`${where}.actual_fees 字段不符合契约`);
    const quantity = source.quantity_shares;
    if (!Number.isSafeInteger(quantity) || Number(quantity) <= 0 || Number(quantity) > 4_294_967_295) throw new Error(`${where}.quantity_shares 必须是正 u32 股数`);
    if (typeof source.civil_date !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(source.civil_date)
      || new Date(`${source.civil_date}T00:00:00.000Z`).toISOString().slice(0, 10) !== source.civil_date) throw new Error(`${where}.civil_date 必须是有效 ISO 自然日`);
    if (typeof source.stock !== "number" || !Number.isInteger(source.stock) || source.stock < 0) throw new Error(`${where}.stock 必须为数值StockId`);
    const code = stockCodeOf(source.stock, `${where}.stock`);
    if (source.side !== "Buy" && source.side !== "Sell") throw new Error(`${where}.side 必须是 Buy 或 Sell`);
    const price = money(source.price_cents, `${where}.price_cents`);
    const gross = money(source.gross_cents, `${where}.gross_cents`);
    if (price === "0" || gross === "0" || BigInt(price) * BigInt(Number(quantity)) !== BigInt(gross)) throw new Error(`${where} 成交价、成交量与成交额不一致`);
    return {
      receipt_id: decimal(source.receipt_id, `${where}.receipt_id`),
      civil_date: source.civil_date,
      code,
      side: source.side,
      price,
      quantity_shares: Number(quantity),
      gross,
      actual_fees: {
        commission: money(fees.commission, `${where}.actual_fees.commission`),
        stamp_tax: money(fees.stamp_tax, `${where}.actual_fees.stamp_tax`),
        transfer_fee: money(fees.transfer_fee, `${where}.actual_fees.transfer_fee`),
      },
    };
  });
}
