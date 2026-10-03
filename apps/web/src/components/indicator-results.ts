export interface OhlcIndicatorInput {
  high: number;
  low: number;
  close: number;
}

export interface IndicatorInput {
  prices: number[];
  candles?: OhlcIndicatorInput[];
}

export interface IndicatorMacd {
  dif: number[];
  dea: number[];
  histogram: number[];
}

export interface IndicatorKdj {
  k: number[];
  d: number[];
  j: number[];
}

export interface IndicatorResults {
  macd: IndicatorMacd;
  priceKdj: IndicatorKdj;
  candleKdj: IndicatorKdj;
}

export type IndicatorCalculator = (input: IndicatorInput) => Promise<IndicatorResults>;

export type IndicatorResultState =
  | { kind: "idle" }
  | { kind: "unavailable" }
  | { kind: "pending" }
  | { kind: "ready"; value: IndicatorResults }
  | { kind: "error"; message: string };

export interface IndicatorResultRecord {
  readonly request: IndicatorResultRequest;
  readonly state: IndicatorResultState;
}

export class IndicatorResultRequest {
  private readonly gate: IndicatorRequestGate;
  private readonly generation: number;
  private readonly calculator: IndicatorCalculator;
  private readonly input: IndicatorInput;

  private constructor(gate: IndicatorRequestGate, calculator: IndicatorCalculator, input: IndicatorInput) {
    this.gate = gate;
    this.generation = gate.capture();
    this.calculator = calculator;
    this.input = input;
  }

  static capture(gate: IndicatorRequestGate, calculator: IndicatorCalculator, input: IndicatorInput): IndicatorResultRequest {
    return new IndicatorResultRequest(gate, calculator, input);
  }

  matches(calculator: IndicatorCalculator, input: IndicatorInput): boolean {
    return this.calculator === calculator && this.input === input;
  }

  pendingRecord(): IndicatorResultRecord {
    return { request: this, state: { kind: "pending" } };
  }

  resolveRecord(response: unknown): IndicatorResultRecord | null {
    if (!this.gate.isCurrent(this.generation)) return null;
    try {
      const value = parseIndicatorResults(response, this.input.prices.length, this.input.candles?.length ?? 0);
      return { request: this, state: { kind: "ready", value } };
    } catch (error) {
      return { request: this, state: { kind: "error", message: describeIndicatorFailure(error) } };
    }
  }

  rejectRecord(error: unknown): IndicatorResultRecord | null {
    if (!this.gate.isCurrent(this.generation)) return null;
    return { request: this, state: { kind: "error", message: describeIndicatorFailure(error) } };
  }
}

function describeIndicatorFailure(error: unknown): string {
  return error instanceof Error ? `${error.name}: ${error.message}` : String(error);
}

export class IndicatorRequestGate {
  private generation = 0;

  capture(): number {
    return this.generation;
  }

  invalidate(): void {
    this.generation += 1;
  }

  isCurrent(generation: number): boolean {
    return generation === this.generation;
  }
}

export function parseIndicatorResults(value: unknown, priceCount: number, candleCount: number): IndicatorResults {
  const root = requireRecord(value, "指标结果");
  const macd = requireRecord(root.macd, "MACD");
  const priceKdj = requireRecord(root.priceKdj, "价格 KDJ");
  const candleKdj = requireRecord(root.candleKdj, "OHLC KDJ");
  return {
    macd: {
      dif: requireSeries(macd.dif, priceCount, "MACD DIF"),
      dea: requireSeries(macd.dea, priceCount, "MACD DEA"),
      histogram: requireSeries(macd.histogram, priceCount, "MACD 柱"),
    },
    priceKdj: {
      k: requireSeries(priceKdj.k, priceCount, "价格 KDJ K"),
      d: requireSeries(priceKdj.d, priceCount, "价格 KDJ D"),
      j: requireSeries(priceKdj.j, priceCount, "价格 KDJ J"),
    },
    candleKdj: {
      k: requireSeries(candleKdj.k, candleCount, "OHLC KDJ K"),
      d: requireSeries(candleKdj.d, candleCount, "OHLC KDJ D"),
      j: requireSeries(candleKdj.j, candleCount, "OHLC KDJ J"),
    },
  };
}

function requireRecord(value: unknown, label: string): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`${label}必须是对象`);
  }
  return value as Record<string, unknown>;
}

function requireSeries(value: unknown, expectedLength: number, label: string): number[] {
  if (!Array.isArray(value) || value.length !== expectedLength) {
    throw new RangeError(`${label}长度必须为 ${expectedLength}`);
  }
  if (!value.every((item) => typeof item === "number" && Number.isFinite(item))) {
    throw new TypeError(`${label}必须只包含有限数值`);
  }
  return value as number[];
}
