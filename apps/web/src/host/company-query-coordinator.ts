import type { EngineHost } from "./engine-host.ts";
import type { PublicReportSummary } from "../types/engine.ts";
import type { CodedEvent } from "./protocol/stock-identity.ts";
import type { NormalizedCivilUpdate, NormalizedTickFrame } from "./protocol/index.ts";
import type { UnknownAction } from "@reduxjs/toolkit";
import {
  advanceCompanyEventCoverage,
  installCompanyBaseline,
  invalidateCompanyForDisclosure,
  recordCivilDateAdvanced,
  recordCompanyPage,
  recordCompanyReport,
  recordCompanyQueryFailure,
  reconcileCompanyPublicMetadata,
  unavailable,
  startCompanyQuery,
} from "../store/company-slice.ts";
import { normalizePublicReportById, normalizePublicReportPage } from "./serde-normalize.ts";
import { CompanyRequestRegistry } from "./company-request-registry.ts";

interface Baseline {
  civilDate: string | null;
  revision: string | null;
  seq: number;
}

/** companyId = CompanyId 数值的 canonical 十进制串；cursor = PublicationId 数值的十进制文本。 */
interface CompanyQuery {
  companyId: string;
  cursor: string | null;
}

type Dispatch = (action: UnknownAction) => unknown;
type CompanyQueryHost = Pick<EngineHost, "capabilities" | "publicReportById" | "queryPublicReports">;

interface PublicMetadata {
  civilDate: string | null;
  revision: string | null;
}

/**
 * 视图层公司身份约定：CompanyId 数值的 canonical 十进制字符串（与 AccountId
 * canonical 化同式；披露事件摄入层 stock-identity.ts 也按此形态输出）。
 * wire 面 `PublicReportQuery.company_id` 是数值（ADR 0041 CompanyId u32），
 * 请求边界在此解析转换，不匹配规范十进制即显式失败。
 */
function wireCompanyId(companyId: string): number {
  if (!/^(0|[1-9]\d*)$/.test(companyId) || !Number.isSafeInteger(Number(companyId))) {
    throw new TypeError(`公司查询身份必须是 CompanyId 的规范十进制串：${companyId}`);
  }
  return Number(companyId);
}

/**
 * 分页游标是 PublicationId（u32）数值的十进制文本（`PublicReportPage.next_cursor`
 * 原样回传）；wire 面 `PublicReportQuery.cursor` 是数值，此处同式解析转换。
 */
function wireCursor(cursor: string | null): number | null {
  if (cursor === null) return null;
  if (!/^(0|[1-9]\d*)$/.test(cursor) || !Number.isSafeInteger(Number(cursor))) {
    throw new TypeError(`公开报告分页游标必须是 PublicationId 的规范十进制串：${cursor}`);
  }
  return Number(cursor);
}

function parsePage(value: unknown, companyId: string): { reports: PublicReportSummary[]; nextCursor: string | null } {
  const expected = wireCompanyId(companyId);
  const page = normalizePublicReportPage(value);
  if (page.reports.some((report) => report.company_id !== expected)) {
    throw new TypeError("公开公司报告响应不符合公共 DTO 契约");
  }
  return { reports: page.reports, nextCursor: page.next_cursor };
}

function parseReport(value: unknown, companyId: string): PublicReportSummary {
  const expected = wireCompanyId(companyId);
  const report = normalizePublicReportById(value);
  if (report.company_id !== expected) {
    throw new TypeError("公开公司报告响应不符合公共 DTO 契约");
  }
  return report;
}

export class CompanyQueryCoordinator {
  private disposed = false;
  private generation = 0;
  private lastEventSeq = 0;
  private readonly requests = new CompanyRequestRegistry();
  private readonly cachedCompanies = new Set<string>();
  private readonly host: CompanyQueryHost;
  private readonly dispatch: Dispatch;

  constructor(host: CompanyQueryHost, dispatch: Dispatch) {
    this.host = host;
    this.dispatch = dispatch;
  }

  dispose(): void {
    this.disposed = true;
    this.requests.clear();
    this.cachedCompanies.clear();
  }

  installBaseline(baseline: Baseline): void {
    if (this.disposed) return;
    this.generation += 1;
    this.lastEventSeq = baseline.seq;
    this.requests.clear();
    this.cachedCompanies.clear();
    this.dispatch(installCompanyBaseline({ generation: this.generation, ...baseline }));
  }

  async query(query: CompanyQuery, force = false): Promise<void> {
    if (this.disposed) return;
    const generation = this.generation;
    const key = `${generation}\u0000${query.companyId}\u0000${query.cursor ?? "root"}`;
    const request = this.requests.begin(key, force);
    if (request === null) return;
    this.cachedCompanies.add(query.companyId);
    if (!this.host.capabilities.publicCompanyReports || !this.host.queryPublicReports) {
      this.dispatch(unavailable({ generation, ...query, message: "当前宿主不支持公开公司报告查询" }));
      this.requests.finishIfCurrent(key, request);
      return;
    }
    this.dispatch(startCompanyQuery({ generation, ...query }));
    try {
      const response = parsePage(await this.host.queryPublicReports({
        company_id: wireCompanyId(query.companyId), cursor: wireCursor(query.cursor), page_size: null,
      }), query.companyId);
      if (this.isCurrentRequest(generation, key, request)) {
        this.dispatch(recordCompanyPage({ generation, ...query, ...response }));
      }
    } catch (error) {
      if (this.isCurrentRequest(generation, key, request)) {
        this.dispatch(recordCompanyQueryFailure({
          generation, ...query, message: error instanceof Error ? error.message : String(error),
        }));
      }
    } finally {
      if (this.isCurrentRequest(generation, key, request)) this.requests.finishIfCurrent(key, request);
    }
  }

  async queryReportById(query: { companyId: string; reportId: string }): Promise<void> {
    if (this.disposed) return;
    const generation = this.generation;
    const key = `${generation}\u0000${query.companyId}\u0000report\u0000${query.reportId}`;
    const request = this.requests.begin(key);
    if (request === null) return;
    this.cachedCompanies.add(query.companyId);
    if (!this.host.capabilities.publicCompanyReports || !this.host.publicReportById) {
      this.dispatch(unavailable({ generation, companyId: query.companyId, cursor: null, message: "当前宿主不支持公开公司报告查询" }));
      this.requests.finishIfCurrent(key, request);
      return;
    }
    try {
      const report = parseReport(await this.host.publicReportById(query.reportId), query.companyId);
      if (this.isCurrentRequest(generation, key, request)) {
        this.dispatch(recordCompanyReport({ generation, companyId: query.companyId, report }));
      }
    } catch (error) {
      if (this.isCurrentRequest(generation, key, request)) {
        this.dispatch(recordCompanyQueryFailure({
          generation, companyId: query.companyId, cursor: null,
          message: error instanceof Error ? error.message : String(error),
        }));
      }
    } finally {
      if (this.isCurrentRequest(generation, key, request)) this.requests.finishIfCurrent(key, request);
    }
  }

  acceptFrame(frame: NormalizedTickFrame, metadata: PublicMetadata = { civilDate: null, revision: null }): void {
    if (this.disposed) return;
    this.acceptEvents(frame.events, frame.seqFrom, frame.seqTo, true);
    this.dispatch(reconcileCompanyPublicMetadata({ generation: this.generation, ...metadata }));
  }

  acceptCivil(update: NormalizedCivilUpdate, metadata: PublicMetadata = { civilDate: null, revision: null }): void {
    if (this.disposed) return;
    this.acceptEvents(update.update.events, update.update.seq_from, update.update.seq_to, false);
    this.dispatch(reconcileCompanyPublicMetadata({ generation: this.generation, ...metadata }));
    for (const companyId of this.cachedCompanies) void this.query({ companyId, cursor: null }, true);
  }

  private acceptEvents(
    events: readonly CodedEvent[],
    seqFrom: number,
    seqTo: number,
    refreshDisclosures: boolean,
  ): void {
    this.assertEventCoverage(events, seqFrom, seqTo);
    for (const event of events) {
      if ("CivilDateAdvanced" in event) {
        this.dispatch(recordCivilDateAdvanced({
          generation: this.generation,
          seq: event.CivilDateAdvanced.seq,
          civilDate: event.CivilDateAdvanced.next_date,
          dayStatus: event.CivilDateAdvanced.next_status,
        }));
      } else if ("CompanyDisclosurePublished" in event) {
        const disclosure = event.CompanyDisclosurePublished;
        this.dispatch(invalidateCompanyForDisclosure({
          generation: this.generation,
          seq: disclosure.seq,
          companyId: disclosure.company,
        }));
        if (refreshDisclosures && this.cachedCompanies.has(disclosure.company)) {
          void this.query({ companyId: disclosure.company, cursor: null }, true);
        }
      }
    }
    this.lastEventSeq = seqTo;
    this.dispatch(advanceCompanyEventCoverage({ generation: this.generation, toSeq: seqTo }));
  }

  private assertEventCoverage(events: readonly CodedEvent[], seqFrom: number, seqTo: number): void {
    if (!Number.isSafeInteger(seqFrom) || !Number.isSafeInteger(seqTo) || seqFrom < 0 || seqTo < seqFrom) {
      throw new Error("公共公司状态事件覆盖区间无效");
    }
    if (seqFrom !== this.lastEventSeq) {
      throw new Error(`公共公司状态需要宿主重同步：期望前序 seq ${this.lastEventSeq}，收到 ${seqFrom}`);
    }
    let previousSeq = seqFrom;
    for (const event of events) {
      const seq = companyEventSeq(event);
      if (seq !== previousSeq + 1 || seq > seqTo) {
        throw new Error("公共公司状态事件与宿主 seq 覆盖区间不一致");
      }
      previousSeq = seq;
    }
    if (previousSeq !== seqTo) {
      throw new Error("公共公司状态事件与宿主 seq 覆盖区间不一致");
    }
  }

  private isCurrentRequest(generation: number, key: string, request: number): boolean {
    return !this.disposed && generation === this.generation && this.requests.matches(key, request);
  }
}

function companyEventSeq(event: CodedEvent): number {
  if ("Trade" in event) return event.Trade.seq;
  if ("AuctionTick" in event) return event.AuctionTick.seq;
  if ("AuctionCompleted" in event) return event.AuctionCompleted.seq;
  if ("PriceTick" in event) return event.PriceTick.seq;
  if ("DayBoundary" in event) return event.DayBoundary.seq;
  if ("CivilDateAdvanced" in event) return event.CivilDateAdvanced.seq;
  if ("CompanyDisclosurePublished" in event) return event.CompanyDisclosurePublished.seq;
  if ("IntentRejected" in event) return event.IntentRejected.seq;
  if ("SettlementError" in event) return event.SettlementError.seq;
  if ("OrderCanceled" in event) return event.OrderCanceled.seq;
  if ("OrderAccepted" in event) return event.OrderAccepted.seq;
  throw new Error(`未处理的公共公司状态事件：${JSON.stringify(event)}`);
}
