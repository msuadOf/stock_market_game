import assert from "node:assert/strict";
import test from "node:test";
import type { EngineEvent } from "../types/engine";
import type { KlinePoint, PricePoint } from "../components/PriceChart";
import {
  MinutePointCollector,
  AuctionPointCollector,
  AUCTION_VOLUME_LINES_PER_MINUTE,
  CALL_AUCTION_ENTRY_MINUTES,
  aggregateCandles,
  buildFiveLevelBook,
  calculateKdj,
  candleBodyPrices,
  candleWickPrices,
  chartSlotGeometry,
  currentTradingDayEvents,
  formatTradingMinute,
  formatGameClock,
  formatTradeLots,
  marketCodesForView,
  orderBookDepthPercent,
  klineWindow,
  mergeMinutePoints,
  reduceKlineViewport,
  updateDailyCandle,
  priceChangePercent,
  sparklineGeometry,
  sparklinePoints,
  symmetricIntradayScale,
  intradayChartX,
  intradayVolumeScale,
  tradingDayProgress,
} from "./market-model.ts";

function priceTick(seq: number, code: string, lastPrice: number, tick = seq, volume = 0): EngineEvent {
  return { PriceTick: {
    seq,
    tick,
    code,
    last_price: lastPrice,
    daily_candle: { time: 0, open: lastPrice, high: lastPrice, low: lastPrice, close: lastPrice, volume },
    bids: [],
    asks: [],
  } };
}

test("五档盘口固定展示十个真实档位槽，缺失档位保持为空", () => {
  const book = buildFiveLevelBook([[1000, 300], [999, 500]], [[1001, 200], [1002, 400]]);
  assert.deepEqual(book.sells.map((slot) => [slot.label, slot.level]), [
    ["卖5", null], ["卖4", null], ["卖3", null], ["卖2", [1002, 400]], ["卖1", [1001, 200]],
  ]);
  assert.deepEqual(book.buys.map((slot) => [slot.label, slot.level]), [
    ["买1", [1000, 300]], ["买2", [999, 500]], ["买3", null], ["买4", null], ["买5", null],
  ]);
});

test("逐笔成交手数精确显示且零碎股不舍入成零", () => {
  assert.equal(formatTradeLots(2, 100), "0.02");
  assert.equal(formatTradeLots(100, 100), "1");
  assert.equal(formatTradeLots(250, 100), "2.5");
  assert.equal(formatTradeLots(999_999, 100), "9999.99");
  assert.equal(formatTradeLots(1_000_000, 100), "1万");
  assert.equal(formatTradeLots(10_000_000_000, 100), "1亿");
  assert.equal(formatTradeLots(100_000_000_000_000, 100), "1万亿");
});

test("分时纵轴按竞价和盘中最大偏离围绕昨收严格对称", () => {
  const scale = symmetricIntradayScale([32.45, 30.01, 30.34], 30.34, 0);

  assert.equal(scale.top, 32.45);
  assert.ok(Math.abs(scale.bottom - 28.23) < 1e-10);
  assert.ok(Math.abs((scale.top - 30.34) - (30.34 - scale.bottom)) < 1e-10);
  assert.equal(scale.topPercent, -scale.bottomPercent);
  assert.ok(scale.bottom <= 30.01 && scale.top >= 32.45);
});

test("连续竞价压缩午休并把全天交易时段等分为四段", () => {
  assert.equal(intradayChartX({ phase: "auction", minute: 0 }), 0);
  assert.equal(CALL_AUCTION_ENTRY_MINUTES, 10);
  assert.equal(intradayChartX({ phase: "auction", minute: 99 }), 16);
  assert.throws(() => intradayChartX({ phase: "auction", minute: 100 }), /集合竞价细线槽/);
  assert.equal(intradayChartX({ phase: "continuous", minute: 0 }), 16);
  assert.ok(Math.abs(intradayChartX({ phase: "continuous", minute: 60 }) - 37.09) < 0.01);
  assert.ok(Math.abs(intradayChartX({ phase: "continuous", minute: 119 }) - 57.82) < 0.01);
  assert.ok(Math.abs(intradayChartX({ phase: "continuous", minute: 120 }) - 58.18) < 0.01);
  assert.ok(intradayChartX({ phase: "continuous", minute: 120 }) - intradayChartX({ phase: "continuous", minute: 119 }) < 0.4);
  assert.ok(Math.abs(intradayChartX({ phase: "continuous", minute: 179 }) - 78.91) < 0.01);
  assert.equal(intradayChartX({ phase: "continuous", minute: 239 }), 100);
});

test("盘口量色块按同侧最大挂单量计算相对宽度", () => {
  assert.equal(orderBookDepthPercent(500, 1000), 50);
  assert.equal(orderBookDepthPercent(1000, 1000), 100);
  assert.equal(orderBookDepthPercent(0, 1000), 0);
  assert.throws(() => orderBookDepthPercent(-1, 1000), /盘口挂单量/);
  assert.throws(() => orderBookDepthPercent(1, 0), /最大挂单量/);
});

test("集合竞价与连续竞价分别归一到相同的量柱显示高度", () => {
  const scale = intradayVolumeScale([120, 480, 960], [8, 16, 24]);

  assert.deepEqual(scale, { auctionMax: 960, continuousMax: 24 });
  assert.equal(480 / scale.auctionMax, 0.5);
  assert.equal(12 / scale.continuousMax, 0.5);
});

test("集合竞价以当前已有最高量柱作为独立纵轴的百分之百", () => {
  const scale = intradayVolumeScale([8, 24, 16], [800, 1_200]);

  assert.equal(scale.auctionMax, 24);
  assert.equal(24 / scale.auctionMax, 1);
  assert.equal(scale.continuousMax, 1_200);
});

test("集合竞价累计量增加时纵轴随当前最高值自适应", () => {
  const early = intradayVolumeScale([36], []);
  const later = intradayVolumeScale([36, 90], []);

  assert.equal(early.auctionMax, 36);
  assert.equal(later.auctionMax, 90);
  assert.equal(90 / later.auctionMax, 1);
});

test("集合竞价指示价按固定六秒槽聚合且不伪造空价格", () => {
  const collector = new AuctionPointCollector("600460", 60, 15_300, 900);
  const events: EngineEvent[] = [
    { AuctionTick: { seq: 1, tick: 1, code: "600460", indicative_price: null, matched_volume: 0, imbalance: 100 } },
    { AuctionTick: { seq: 2, tick: 60, code: "600460", indicative_price: 3030, matched_volume: 200, imbalance: 50 } },
    { AuctionTick: { seq: 3, tick: 61, code: "600460", indicative_price: 3040, matched_volume: 300, imbalance: 20 } },
  ];

  assert.deepEqual(collector.collect(events), [
    { time: 0, value: null, volume: 0, buy: false },
    { time: 9, value: 30.3, volume: 200, buy: true },
    { time: 10, value: 30.4, volume: 300, buy: true },
  ]);
});

test("第二和第三个交易日即使没有指示价也保留真实集合竞价量槽", () => {
  const collector = new AuctionPointCollector("600460", 60, 15_300, 900);
  const auctionTick = (tick: number, matchedVolume: number): EngineEvent => ({
    AuctionTick: {
      seq: tick,
      tick,
      code: "600460",
      indicative_price: null,
      matched_volume: matchedVolume,
      imbalance: 100,
    },
  });

  assert.deepEqual(collector.collect([auctionTick(15_301, 0)]), [
    { time: 0, value: null, volume: 0, buy: false },
  ]);
  assert.deepEqual(collector.collect([auctionTick(30_601, 0)]), [
    { time: 0, value: null, volume: 0, buy: false },
  ]);
});

test("集合竞价累计量按每分钟十根细线随权威时间向右推进", () => {
  const collector = new AuctionPointCollector("600460", 60, 15_300, 900);
  const events: EngineEvent[] = [
    { AuctionTick: { seq: 1, tick: 1, code: "600460", indicative_price: 3030, matched_volume: 100, imbalance: 50 } },
    { AuctionTick: { seq: 2, tick: 6, code: "600460", indicative_price: 3035, matched_volume: 180, imbalance: 20 } },
    { AuctionTick: { seq: 3, tick: 7, code: "600460", indicative_price: 3040, matched_volume: 240, imbalance: 10 } },
  ];

  assert.equal(AUCTION_VOLUME_LINES_PER_MINUTE, 10);
  assert.deepEqual(collector.collect(events), [
    { time: 0, value: 30.35, volume: 180, buy: true },
    { time: 1, value: 30.4, volume: 240, buy: true },
  ]);
});

test("09:25 的竞价结果占满集合竞价区且不绘制盘前静默期", () => {
  const collector = new AuctionPointCollector("600460", 60, 15_300, 900);
  const points = collector.collect([
    { AuctionTick: { seq: 1, tick: 600, code: "600460", indicative_price: 3040, matched_volume: 2_400, imbalance: 0 } },
    { AuctionCompleted: { seq: 2, tick: 600, code: "600460", opening_price: 3040, matched_volume: 2_400 } },
  ]);

  assert.equal(points[0].time, 99);
  assert.equal(points.at(-1)?.time, 99);
  assert.deepEqual(points.at(-1), {
    time: 99,
    value: 30.4,
    volume: 2_400,
    buy: true,
  });
  assert.throws(() => collector.collect([
    { AuctionTick: { seq: 3, tick: 601, code: "600460", indicative_price: 3040, matched_volume: 2_400, imbalance: 0 } },
  ]), /超出开盘集合竞价阶段/);
});

test("连续分时从集合竞价结束后的 09:30 槽位重新计分钟", () => {
  const collector = new MinutePointCollector("600460", 60, 15_300, 900);
  const events = [
    priceTick(1, "600460", 3030, 901),
    priceTick(2, "600460", 3040, 960),
    priceTick(3, "600460", 3050, 961),
  ];

  assert.deepEqual(collector.collect(events).map(({ time, value }) => ({ time, value })), [
    { time: 0, value: 30.4 },
    { time: 1, value: 30.5 },
  ]);
});

test("09:30 连续竞价首分钟量不重复计入集合竞价成交量", () => {
  const collector = new MinutePointCollector("600460", 60, 15_300, 900);
  const events: EngineEvent[] = [
    { AuctionCompleted: { seq: 1, tick: 900, code: "600460", opening_price: 3030, matched_volume: 529_070 } },
    priceTick(2, "600460", 3030, 901, 529_070),
    { Trade: { seq: 3, code: "600460", price: 3031, qty: 100, maker: 1, taker: 2 } },
    priceTick(4, "600460", 3031, 902, 529_170),
  ];

  assert.deepEqual(collector.collect(events), [
    { time: 0, value: 30.31, volume: 100, buy: true },
  ]);
});

test("同一分钟跨事件批次持续更新同一根分时点和量柱", () => {
  const collector = new MinutePointCollector("600460");
  const firstSecond: EngineEvent[] = [
    { Trade: { seq: 1, code: "600460", price: 3030, qty: 200, maker: 1, taker: 2 } },
    priceTick(2, "600460", 3030, 1, 200),
  ];
  const remainingSeconds: EngineEvent[] = Array.from({ length: 59 }, (_, index) =>
    priceTick(index + 3, "600460", 3031, index + 2, 200));

  assert.deepEqual(collector.collect(firstSecond), [
    { time: 0, value: 30.3, volume: 200, buy: true },
  ]);
  assert.deepEqual(collector.collect(remainingSeconds), [
    { time: 0, value: 30.31, volume: 200, buy: true },
  ]);
});

test("高倍率丢弃中间逐秒事件后仍按权威 tick 推进到对应交易分钟", () => {
  const collector = new MinutePointCollector("600460");
  const events = [
    priceTick(10, "600460", 3030, 1),
    priceTick(20, "600460", 3031, 60),
    priceTick(30, "600460", 3032, 61),
    priceTick(40, "600460", 3033, 120),
  ];

  assert.deepEqual(collector.collect(events).map(({ time, value }) => ({ time, value })), [
    { time: 0, value: 30.31 },
    { time: 1, value: 30.33 },
  ]);
});

test("同一分钟的新采样替换旧采样而不是制造重复分钟", () => {
  const collector = new MinutePointCollector("600460");
  const history = collector.collect([priceTick(1, "600460", 3030, 1)]);
  const update = collector.collect([priceTick(2, "600460", 3035, 30)]);

  assert.deepEqual(mergeMinutePoints(history, update).map(({ time, value }) => ({ time, value })), [
    { time: 0, value: 30.35 },
  ]);
});

test("高倍率批次跨日时只把最后日界后的事件交给新日分时", () => {
  const boundary: EngineEvent = {
    DayBoundary: { seq: 2, day: 1, closed_daily_candles: {} },
  };
  const oldDay = priceTick(1, "600460", 3030, 14_400);
  const newDay = priceTick(3, "600460", 3040, 14_401);

  assert.deepEqual(currentTradingDayEvents([oldDay, boundary, newDay]), [newDay]);
});

test("其它股票的成交量不会混入当前股票", () => {
  const collector = new MinutePointCollector("600460", 1);
  const events: EngineEvent[] = [
    { Trade: { seq: 1, code: "000001", price: 1000, qty: 900, maker: 1, taker: 2 } },
    priceTick(2, "000001", 1000),
    priceTick(3, "600460", 3034),
  ];

  assert.deepEqual(collector.collect(events), [
    { time: 2, value: 30.34, volume: 0, buy: false },
  ]);
});

test("一分钟内最后成交价决定量柱方向", () => {
  const collector = new MinutePointCollector("600460");
  const events: EngineEvent[] = [
    { Trade: { seq: 7, code: "600460", price: 3033, qty: 200, maker: 1, taker: 2 } },
    { Trade: { seq: 8, code: "600460", price: 3034, qty: 300, maker: 3, taker: 4 } },
    priceTick(9, "600460", 3034, 1, 500),
  ];

  assert.deepEqual(collector.collect(events), [
    { time: 0, value: 30.34, volume: 500, buy: true },
  ]);
});

test("交易进度限制在 0 到 1，时间跨过午间休市", () => {
  assert.equal(tradingDayProgress(0, 240), 0);
  assert.equal(tradingDayProgress(178, 240), 178 / 240);
  assert.equal(tradingDayProgress(999, 240), 1);
  assert.equal(formatTradingMinute(0), "09:30");
  assert.equal(formatTradingMinute(119), "11:29");
  assert.equal(formatTradingMinute(120), "13:00");
  assert.equal(formatTradingMinute(239), "14:59");
});

test("游戏时钟由权威 tick 换算并跳过午间休市", () => {
  assert.equal(formatGameClock(0), "09:15:00");
  assert.equal(formatGameClock(899), "09:29:59");
  assert.equal(formatGameClock(900), "09:30:00");
  assert.equal(formatGameClock(8_099), "11:29:59");
  assert.equal(formatGameClock(8_100), "13:00:00");
  assert.equal(formatGameClock(15_299), "14:59:59");
  assert.equal(formatGameClock(15_300), "09:15:00");
  assert.throws(() => formatGameClock(-1), /tick/);
  assert.throws(() => formatGameClock(1.5), /tick/);
});

test("行情列表包含快照里的动态股票，并按真实持仓筛选", () => {
  const marketCodes = ["999999", "000001", "600460"];
  const preferredOrder = ["600460", "000001"];

  assert.deepEqual(marketCodesForView(marketCodes, preferredOrder, "watchlist", new Set()), [
    "600460", "000001", "999999",
  ]);
  assert.deepEqual(marketCodesForView(marketCodes, preferredOrder, "holdings", new Set(["999999"])), [
    "999999",
  ]);
});

test("涨跌幅始终以前一交易日收盘价为分母", () => {
  assert.equal(priceChangePercent(1_100, 1_000), 10);
  assert.equal(priceChangePercent(1_155, 1_100), 5);
  assert.equal(priceChangePercent(1_000, 0), 0);
});

test("迷你走势图只由真实价格历史生成", () => {
  const points: PricePoint[] = [
    { time: 1, value: 10 },
    { time: 2, value: 20 },
    { time: 3, value: 10 },
  ];
  assert.equal(sparklinePoints(points, 64, 48), "0.27,48 0.54,0 0.8,48");
  assert.deepEqual(sparklineGeometry([
    { time: 1, value: 10 },
    { time: 2, value: 11 },
    { time: 3, value: 10.5 },
  ], 10, 64, 48), {
    linePoints: "0.27,43.35 0.54,4.65 0.8,24",
    areaPoints: "0.27,43.35 0.27,43.35 0.54,4.65 0.8,24 0.8,43.35",
    axisY: 43.35,
  });
  assert.equal(sparklinePoints(points.slice(0, 1), 64, 48), "");
  assert.deepEqual(sparklineGeometry(points.slice(0, 1), 10, 64, 48), {
    linePoints: "",
    areaPoints: "",
    axisY: 24,
  });

  const early = sparklineGeometry([
    { time: 0, value: 10 },
    { time: 1, value: 11 },
    { time: 2, value: 10.5 },
  ], 10, 64, 48);
  const afternoon = sparklineGeometry([
    { time: 120, value: 10 },
    { time: 180, value: 11 },
    { time: 239, value: 10.5 },
  ], 10, 64, 48);
  assert.equal(early.linePoints, "0,43.35 0.27,4.65 0.54,24");
  assert.equal(afternoon.linePoints, "32.13,43.35 48.2,4.65 64,24");
  assert.notDeepEqual(afternoon, early, "缩略图使用固定分钟槽位，不得按现有点数重新缩放横轴");
  assert.throws(
    () => sparklineGeometry([{ time: 240, value: 10 }, { time: 241, value: 11 }], 10, 64, 48),
    /分钟槽位/,
  );
});

test("周 K 和月 K 按真实交易日 OHLCV 聚合", () => {
  const candles: KlinePoint[] = Array.from({ length: 6 }, (_, index) => ({
    time: (index + 1) as KlinePoint["time"],
    open: 10 + index,
    high: 12 + index,
    low: 9 + index,
    close: 11 + index,
    volume: 100 * (index + 1),
  }));

  assert.deepEqual(aggregateCandles(candles, "周K"), [
    { time: 1, open: 10, high: 16, low: 9, close: 15, volume: 1500 },
    { time: 6, open: 15, high: 17, low: 14, close: 16, volume: 600 },
  ]);
  assert.deepEqual(aggregateCandles(candles, "日K"), candles);
});

test("标准 KDJ 在无波动行情保持 K、D、J 为 50", () => {
  const candles: KlinePoint[] = Array.from({ length: 12 }, (_, index) => ({
    time: (index + 1) as KlinePoint["time"], open: 10, high: 10, low: 10, close: 10,
  }));
  const result = calculateKdj(candles);
  assert.deepEqual(result.k, Array(12).fill(50));
  assert.deepEqual(result.d, Array(12).fill(50));
  assert.deepEqual(result.j, Array(12).fill(50));
});

test("日 K、成交量与指标共轴，最大放大时以紧凑槽位铺满横轴", () => {
  const slotWidth = 390 / 72;
  assert.deepEqual(chartSlotGeometry(0, 3, 390, 72), { center: 2.7083333333333335, markWidth: slotWidth * 0.7 });
  assert.deepEqual(chartSlotGeometry(1, 3, 390, 72), { center: 8.125, markWidth: slotWidth * 0.7 });
  assert.deepEqual(chartSlotGeometry(2, 3, 390, 72), { center: 13.541666666666668, markWidth: slotWidth * 0.7 });
  assert.deepEqual(chartSlotGeometry(71, 72, 390, 72), { center: 387.2916666666667, markWidth: slotWidth * 0.7 });
  const first = chartSlotGeometry(0, 3, 390, 72);
  const second = chartSlotGeometry(1, 3, 390, 72);
  assert.ok(Math.abs((second.center - second.markWidth / 2) - (first.center + first.markWidth / 2) - slotWidth * 0.3) < Number.EPSILON * 8);
  const zoomedSlotWidth = 390 / 30;
  const zoomedFirst = chartSlotGeometry(0, 30, 390, 30);
  const zoomedSecond = chartSlotGeometry(1, 30, 390, 30);
  const zoomedLast = chartSlotGeometry(29, 30, 390, 30);
  assert.deepEqual(zoomedFirst, { center: zoomedSlotWidth / 2, markWidth: 10 });
  assert.deepEqual(zoomedSecond, { center: zoomedSlotWidth * 1.5, markWidth: 10 });
  assert.equal(zoomedSecond.center - zoomedSecond.markWidth / 2 - (zoomedFirst.center + zoomedFirst.markWidth / 2), 3);
  assert.equal(390 - (zoomedLast.center + zoomedLast.markWidth / 2), 1.5, "最大放大时末柱只保留半个柱间距的边缘留白");
  assert.throws(() => chartSlotGeometry(10, 10, 390), /index/);
  assert.throws(() => chartSlotGeometry(2, 3, 390, 2), /capacity/);
});

test("K 线窗口支持缩放、左右移动、最早历史与复位", () => {
  assert.deepEqual(klineWindow(120, 72, 0), {
    start: 48, end: 120, capacity: 72, offsetFromEnd: 0, maxOffset: 48,
  });
  const zoomed = reduceKlineViewport({ capacity: 72, offsetFromEnd: 0 }, 120, "zoom-in");
  assert.deepEqual(zoomed, { capacity: 48, offsetFromEnd: 0 });
  const maximallyZoomed = reduceKlineViewport(zoomed, 120, "zoom-in");
  assert.deepEqual(maximallyZoomed, { capacity: 30, offsetFromEnd: 0 });
  assert.deepEqual(reduceKlineViewport(maximallyZoomed, 120, "zoom-in"), maximallyZoomed);
  const older = reduceKlineViewport(zoomed, 120, "pan-left");
  assert.deepEqual(older, { capacity: 48, offsetFromEnd: 12 });
  assert.deepEqual(reduceKlineViewport(older, 120, "pan-right"), { capacity: 48, offsetFromEnd: 0 });
  assert.deepEqual(reduceKlineViewport(zoomed, 120, "earliest"), { capacity: 48, offsetFromEnd: 72 });
  assert.deepEqual(reduceKlineViewport(older, 120, "reset"), { capacity: 72, offsetFromEnd: 0 });
  assert.throws(() => reduceKlineViewport({ capacity: 8, offsetFromEnd: 0 }, 4, "zoom-in"), /unsupported/);
});

test("日 K 用逐笔成交高低价保留真实上下影线", () => {
  const day = 3 as KlinePoint["time"];
  let candle = updateDailyCandle(undefined, day, 10, 100);
  candle = updateDailyCandle(candle, day, 12, 200);
  candle = updateDailyCandle(candle, day, 9, 300);
  candle = updateDailyCandle(candle, day, 11, 300);
  assert.deepEqual(candle, { time: 3, open: 10, high: 12, low: 9, close: 11, volume: 300 });
});

test("跳空高开低开时实体边界只取当日开盘价与收盘价", () => {
  const previousClose = 10;
  const gapUp = { time: 4 as KlinePoint["time"], open: 12, high: 13, low: 11, close: 11.5 };
  const gapDown = { time: 5 as KlinePoint["time"], open: 8, high: 9.5, low: 7.5, close: 9 };

  assert.deepEqual(candleBodyPrices(gapUp), { top: 12, bottom: 11.5 });
  assert.ok(candleBodyPrices(gapUp).bottom > previousClose, "高开缺口必须保留");
  assert.deepEqual(candleBodyPrices(gapDown), { top: 9, bottom: 8 });
  assert.ok(candleBodyPrices(gapDown).top < previousClose, "低开缺口必须保留");
});

test("日 K 影线在实体边界截断，不贯穿空心红柱", () => {
  const rise = { time: 6 as KlinePoint["time"], open: 10, high: 13, low: 9, close: 12 };

  assert.deepEqual(candleWickPrices(rise), {
    upper: { start: 13, end: 12 },
    lower: { start: 10, end: 9 },
  });
});
