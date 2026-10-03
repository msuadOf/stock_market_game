import { useCallback, useMemo, useState, type MutableRefObject } from "react";
import type { EngineHost } from "../host/engine-host.ts";
import type { AccountSnap, Intent, SessionSetup } from "../types/engine.ts";
import { STOCK_LIST } from "../config/defaults.ts";
import { store, addAutoOrder } from "../store/store.ts";
import { AUTO_ORDER_LABELS, type AutoOrderManager, type AutoOrderType } from "../components/auto-order-manager.ts";
import type { PlayerOrderRefreshGate, PlayerWorkingOrder } from "../components/player-orders.ts";
import { maxAShareOrderQuantity, parseShareQuantity, parseYuanPrice, validateAShareQuantity } from "../utils/trade-input.ts";
import { buildPlayerOrderIntent, playerOrderDescription, type LimitPriceChoice } from "../utils/symbolic-limit-order.ts";

interface Options {
  hostRef: MutableRefObject<EngineHost | null>;
  playerOrderRefreshGateRef: MutableRefObject<PlayerOrderRefreshGate>;
  autoOrderMgrRef: MutableRefObject<AutoOrderManager | null>;
  activeSetup: SessionSetup;
  playerAccount: AccountSnap | null;
  protocolPlayerOrders: Readonly<Record<number, PlayerWorkingOrder>>;
  playerOrdersReady: boolean;
  setNotice(notice: string): void;
}

/** 聚合委托表单、查询列表与取消中状态；engine 与 Redux 仍为交易事实权威。 */
export function useTradingCommands({ hostRef, playerOrderRefreshGateRef, autoOrderMgrRef, activeSetup, playerAccount, protocolPlayerOrders, playerOrdersReady, setNotice }: Options) {
  // 委托面板状态
  const [tradeCode, setTradeCode] = useState<string>(STOCK_LIST[0].code);
  const [orderKind, setOrderKind] = useState<"limit" | "market">("limit");
  const [priceChoice, setPriceChoice] = useState<LimitPriceChoice>("fixed");
  const [priceText, setPriceText] = useState<string>("");
  const [qtyText, setQtyText] = useState<string>("100");
  const [queriedPlayerOrders, setPlayerOrders] = useState<readonly PlayerWorkingOrder[]>([]);
  const playerOrders = useMemo(() => playerOrdersReady
    ? Object.values(protocolPlayerOrders).sort((left, right) => left.id - right.id)
    : queriedPlayerOrders, [playerOrdersReady, protocolPlayerOrders, queriedPlayerOrders]);
  const [cancelingOrderIds, setCancelingOrderIds] = useState<ReadonlySet<number>>(new Set());

  const refreshPlayerOrders = useCallback(async () => {
    const host = hostRef.current;
    if (!host) return;
    if (store.getState().snapshot.playerOrdersReady) return;
    const generation = playerOrderRefreshGateRef.current.next();
    try {
      const orders = await host.playerWorkingOrders();
      if (playerOrderRefreshGateRef.current.isCurrent(generation) && host === hostRef.current) {
        setPlayerOrders(orders);
      }
    } catch (refreshError) {
      if (playerOrderRefreshGateRef.current.isCurrent(generation) && host === hostRef.current) {
        setNotice(`活动委托刷新失败：${refreshError instanceof Error ? refreshError.message : String(refreshError)}`);
      }
    }
  }, [setNotice, hostRef, playerOrderRefreshGateRef]);

  // 自动单添加表单状态
  const [autoType, setAutoType] = useState<AutoOrderType>("stopProfit");
  const [autoTrigger, setAutoTrigger] = useState<string>("");
  const [autoQty, setAutoQty] = useState<string>("100");

  function buildIntent(side: "Buy" | "Sell"): Intent | null {
    try {
      const qty = parseShareQuantity(qtyText);
      const position = playerAccount?.positions[tradeCode];
      const reserved = playerAccount?.reserved_sell_qty[tradeCode] ?? 0;
      const sellable = position ? Math.max(0, position.qty - position.t1_locked - reserved) : 0;
      const stock = activeSetup.stocks.find((candidate) => candidate.code === tradeCode);
      if (!stock) throw new Error(`缺少股票 ${tradeCode} 的 A 股规则配置`);
      validateAShareQuantity(side, qty, sellable, maxAShareOrderQuantity(stock.category, orderKind === "market"));
      return buildPlayerOrderIntent(tradeCode, side, qty, orderKind, priceChoice, priceText);
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
      return null;
    }
  }

  async function submit(side: "Buy" | "Sell") {
    const intent = buildIntent(side);
    if (!intent) return;
    try {
      const currentHost = hostRef.current;
      if (!currentHost) throw new Error("游戏引擎尚未就绪");
      await currentHost.submitIntent(intent);
      const kindText = playerOrderDescription(orderKind, priceChoice, priceText);
      setNotice(`已提交${side === "Buy" ? "买入" : "卖出"}${kindText}委托：${tradeCode} ${qtyText} 股`);
    } catch (e) { setNotice(e instanceof Error ? e.message : String(e)); }
  }

  async function cancelPlayerOrder(order: PlayerWorkingOrder) {
    const host = hostRef.current;
    if (!host) {
      setNotice("游戏引擎尚未就绪，无法撤销委托");
      return;
    }
    setCancelingOrderIds((current) => new Set(current).add(order.id));
    try {
      await host.submitIntent({ Cancel: { code: order.code, id: order.id } });
      setNotice(`已提交撤单请求：委托 #${order.id}`);
    } catch (cancelError) {
      setCancelingOrderIds((current) => {
        const next = new Set(current);
        next.delete(order.id);
        return next;
      });
      setNotice(`撤单请求未入队：${cancelError instanceof Error ? cancelError.message : String(cancelError)}`);
    }
  }

  function addAuto() {
    const side: "Buy" | "Sell" = (autoType === "stopProfit" || autoType === "stopLoss" || autoType === "sellTrigger") ? "Sell" : "Buy";
    let tp: number;
    let qty: number;
    try {
      tp = parseYuanPrice(autoTrigger);
      qty = parseShareQuantity(autoQty);
      const position = playerAccount?.positions[tradeCode];
      const reserved = playerAccount?.reserved_sell_qty[tradeCode] ?? 0;
      const sellable = position ? Math.max(0, position.qty - position.t1_locked - reserved) : 0;
      const stock = activeSetup.stocks.find((candidate) => candidate.code === tradeCode);
      if (!stock) throw new Error(`缺少股票 ${tradeCode} 的 A 股规则配置`);
      validateAShareQuantity(side, qty, sellable, maxAShareOrderQuantity(stock.category));
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
      return;
    }
    const manager = autoOrderMgrRef.current;
    if (!manager) {
      setNotice("游戏引擎尚未就绪，无法添加条件单");
      return;
    }
    const order = manager.add({ code: tradeCode, type: autoType, triggerPrice: tp, qty, side, enabled: true });
    store.dispatch(addAutoOrder(order));
    setNotice(`已添加条件单：${AUTO_ORDER_LABELS[autoType]} ${tradeCode} @ ${autoTrigger} 元`);
  }

  const clearPlayerOrders = useCallback(() => setPlayerOrders([]), []);
  const clearCancelingOrderIds = useCallback(() => setCancelingOrderIds(new Set()), []);
  return {
    form: { tradeCode, orderKind, priceChoice, priceText, qtyText, autoType, autoTrigger, autoQty },
    setTradeCode, setOrderKind, setPriceChoice, setPriceText, setQtyText, setAutoType, setAutoTrigger, setAutoQty,
    playerOrders, cancelingOrderIds, clearPlayerOrders, clearCancelingOrderIds,
    buildIntent, submit, cancelPlayerOrder, addAuto, refreshPlayerOrders,
  };
}
