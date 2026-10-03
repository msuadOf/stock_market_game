import { diffMarketRows, type MarketGridRow, type MarketRowTransaction } from "./market-grid-rows.ts";

export interface MarketGridTransactionTarget {
  applyTransactionAsync(transaction: MarketRowTransaction): void;
}

export class MarketGridRowSynchronizer {
  private api: MarketGridTransactionTarget | null = null;
  private latestRows: readonly MarketGridRow[];
  private submittedRows: readonly MarketGridRow[];

  constructor(initialRows: readonly MarketGridRow[]) {
    this.latestRows = initialRows;
    this.submittedRows = initialRows;
  }

  updateLatest(rows: readonly MarketGridRow[]): void {
    this.recordLatest(rows);
    this.submitLatest();
  }

  /** render 只登记目标；提交仍由 effect 或 ready 驱动。 */
  recordLatest(rows: readonly MarketGridRow[]): void {
    this.latestRows = rows;
  }

  attach(api: MarketGridTransactionTarget, initialRows: readonly MarketGridRow[]): void {
    this.api = api;
    this.submittedRows = initialRows;
    this.submitLatest();
  }

  dispose(): void {
    // AG Grid 自行管理已排队事务；这里只解除借用的 API。
    this.api = null;
  }

  private submitLatest(): void {
    if (this.api === null) return;
    const transaction = diffMarketRows(this.submittedRows, this.latestRows);
    if (transaction.add.length > 0 || transaction.update.length > 0 || transaction.remove.length > 0) {
      this.api.applyTransactionAsync(transaction);
    }
    // submittedRows 记录已提交目标，不表示 AG Grid 已完成异步事务。
    this.submittedRows = this.latestRows;
  }
}
