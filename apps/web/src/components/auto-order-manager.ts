/**
 * 客户端条件单执行器。Redux 负责展示状态，manager 负责按行情事件触发；两者共享 manager
 * 生成的同一个 ID，触发后通过回调把状态写回 Redux。
 */
import type { EngineEvent, Intent, Snapshot, Cents } from "../types/engine";

export type AutoOrderType = "stopProfit" | "stopLoss" | "buyTrigger" | "sellTrigger";

export interface AutoOrder {
  id: string;
  code: string;
  type: AutoOrderType;
  triggerPrice: Cents;
  qty: number;
  side: "Buy" | "Sell";
  enabled: boolean;
  triggered: boolean;
}

export const AUTO_ORDER_LABELS: Record<AutoOrderType, string> = {
  stopProfit: "止盈",
  stopLoss: "止损",
  buyTrigger: "买入触发",
  sellTrigger: "卖出触发",
};

let nextId = 1;

export class AutoOrderManager {
  private orders: AutoOrder[] = [];
  private readonly pending = new Set<string>();
  private readonly onSubmit: (intent: Intent) => void | Promise<void>;
  private readonly onTriggered: (id: string) => void;
  private readonly onSubmitFailed: (id: string, error: unknown) => void;

  constructor(
    onSubmit: (intent: Intent) => void | Promise<void>,
    onTriggered: (id: string) => void = () => {},
    onSubmitFailed: (id: string, error: unknown) => void = () => {},
  ) {
    this.onSubmit = onSubmit;
    this.onTriggered = onTriggered;
    this.onSubmitFailed = onSubmitFailed;
  }

  add(order: Omit<AutoOrder, "id" | "triggered">): AutoOrder {
    const full: AutoOrder = { ...order, id: `auto-${nextId++}`, triggered: false };
    this.orders.push(full);
    return { ...full };
  }

  remove(id: string): void {
    const index = this.orders.findIndex((order) => order.id === id);
    if (index < 0) throw new Error(`找不到条件单：${id}`);
    this.orders.splice(index, 1);
  }

  toggle(id: string): void {
    const order = this.orders.find((candidate) => candidate.id === id);
    if (!order) throw new Error(`找不到条件单：${id}`);
    order.enabled = !order.enabled;
  }

  clearTriggered(): void {
    this.orders = this.orders.filter((order) => !order.triggered);
  }

  clear(): void {
    this.orders = [];
    this.pending.clear();
  }

  list(): AutoOrder[] {
    return this.orders.map((order) => ({ ...order }));
  }

  async checkEvents(events: EngineEvent[], _snapshot: Snapshot): Promise<void> {
    const submissions: Promise<void>[] = [];
    for (const event of events) {
      if (!("PriceTick" in event)) continue;
      const tick = event.PriceTick;
      for (const order of this.orders) {
        if (!order.enabled || order.triggered || this.pending.has(order.id) || order.code !== tick.code) continue;
        const price = tick.last_price;
        const shouldTrigger = order.type === "stopProfit" || order.type === "sellTrigger"
          ? price >= order.triggerPrice
          : price <= order.triggerPrice;
        if (!shouldTrigger) continue;

        this.pending.add(order.id);
        submissions.push(this.submit(order, price));
      }
    }
    await Promise.all(submissions);
  }

  private async submit(order: AutoOrder, price: Cents): Promise<void> {
    try {
      await this.onSubmit({
        PlaceLimit: { code: order.code, side: order.side, price, qty: order.qty },
      });
      const current = this.orders.find((candidate) => candidate.id === order.id);
      if (current) {
        current.triggered = true;
        this.onTriggered(order.id);
      }
    } catch (error) {
      this.onSubmitFailed(order.id, error);
    } finally {
      this.pending.delete(order.id);
    }
  }
}
