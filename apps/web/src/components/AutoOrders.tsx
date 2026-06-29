/**
 * 自动单 / 条件单（客户端侧，不依赖 engine）。
 *
 * 监听 PriceTick 事件 → 当价格满足条件时自动 submitIntent。
 * 支持类型：止盈（价格 >= 触发价 → 卖出）、止损（价格 <= 触发价 → 卖出）、
 *           买入触发（价格 <= 触发价 → 买入）、卖出触发（价格 >= 触发价 → 卖出）。
 *
 * 用法：App.tsx 在事件回调中调 AutoOrderManager.checkEvents(events, snapshot)；
 *       UI 在委托面板里提供「添加条件单」入口。
 */
import type { EngineEvent, Intent, Snapshot, Cents } from "../types/engine";

export type AutoOrderType = "stopProfit" | "stopLoss" | "buyTrigger" | "sellTrigger";

export interface AutoOrder {
  id: string;
  code: string;
  type: AutoOrderType;
  triggerPrice: Cents; // 分
  qty: number;
  side: "Buy" | "Sell";
  enabled: boolean;
  triggered: boolean;
}

/** 中文标签。 */
export const AUTO_ORDER_LABELS: Record<AutoOrderType, string> = {
  stopProfit: "止盈",
  stopLoss: "止损",
  buyTrigger: "买入触发",
  sellTrigger: "卖出触发",
};

let nextId = 1;

export class AutoOrderManager {
  private orders: AutoOrder[] = [];
  private onSubmit: (intent: Intent) => void;

  constructor(onSubmit: (intent: Intent) => void) {
    this.onSubmit = onSubmit;
  }

  /** 添加一条条件单。 */
  add(order: Omit<AutoOrder, "id" | "triggered">): AutoOrder {
    const full: AutoOrder = {
      ...order,
      id: `auto-${nextId++}`,
      triggered: false,
    };
    this.orders.push(full);
    return full;
  }

  /** 删除一条。 */
  remove(id: string): void {
    this.orders = this.orders.filter((o) => o.id !== id);
  }

  /** 启用/禁用。 */
  toggle(id: string): void {
    const o = this.orders.find((x) => x.id === id);
    if (o) o.enabled = !o.enabled;
  }

  /** 清除已触发的。 */
  clearTriggered(): void {
    this.orders = this.orders.filter((o) => !o.triggered);
  }

  list(): AutoOrder[] {
    return [...this.orders];
  }

  /** 在事件回调中调用：检查价格是否触发条件单。 */
  checkEvents(events: EngineEvent[], _snapshot: Snapshot): void {
    for (const ev of events) {
      if (!("PriceTick" in ev)) continue;
      const tick = ev.PriceTick;
      for (const order of this.orders) {
        if (!order.enabled || order.triggered) continue;
        if (order.code !== tick.code) continue;
        const price = tick.last_price;
        let shouldTrigger = false;
        switch (order.type) {
          case "stopProfit":
            shouldTrigger = price >= order.triggerPrice;
            break;
          case "stopLoss":
            shouldTrigger = price <= order.triggerPrice;
            break;
          case "buyTrigger":
            shouldTrigger = price <= order.triggerPrice;
            break;
          case "sellTrigger":
            shouldTrigger = price >= order.triggerPrice;
            break;
        }
        if (shouldTrigger) {
          order.triggered = true;
          this.onSubmit({
            PlaceLimit: {
              code: order.code,
              side: order.side,
              price,
              qty: order.qty,
            },
          });
        }
      }
    }
  }
}
