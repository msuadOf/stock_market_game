import { createSlice, type PayloadAction } from "@reduxjs/toolkit";
import type { PublicReportSummary } from "../types/engine.ts";
import type { DayStatus } from "../types/generated/DayStatus.ts";

type CompanyPage =
  | { kind: "loading" }
  | { kind: "ready"; reportIds: string[]; nextCursor: string | null }
  | { kind: "empty" }
  | { kind: "error"; message: string }
  | { kind: "unavailable"; message: string };

interface CompanyCache {
  reports: Record<string, PublicReportSummary>;
  pages: Record<string, CompanyPage>;
  currentReportId: string | null;
  revision: string;
}

interface CompanyState {
  generation: number;
  civilDate: string | null;
  dayStatus: DayStatus | null;
  revision: string | null;
  lastEventSeq: number;
  selectedCompanyId: string | null;
  companies: Record<string, CompanyCache>;
}

interface BaselinePayload {
  generation: number;
  civilDate: string | null;
  revision: string | null;
  seq: number;
}

interface QueryPayload {
  generation: number;
  companyId: string;
  cursor: string | null;
}

interface PagePayload extends QueryPayload {
  reports: PublicReportSummary[];
  nextCursor: string | null;
}

interface ReportPayload {
  generation: number;
  companyId: string;
  report: PublicReportSummary;
}

interface FailurePayload extends QueryPayload {
  message: string;
}

interface CivilDatePayload {
  generation: number;
  seq: number;
  civilDate: string;
  dayStatus: DayStatus;
}

interface DisclosurePayload {
  generation: number;
  seq: number;
  companyId: string;
}

interface EventCoveragePayload {
  generation: number;
  toSeq: number;
}

interface PublicMetadataPayload {
  generation: number;
  civilDate: string | null;
  revision: string | null;
}

const initialState: CompanyState = {
  generation: 0,
  civilDate: null,
  dayStatus: null,
  revision: null,
  lastEventSeq: 0,
  selectedCompanyId: null,
  companies: {},
};

function pageKey(cursor: string | null): string {
  return cursor ?? "root";
}

function companyCache(state: CompanyState, companyId: string): CompanyCache {
  const existing = state.companies[companyId];
  if (existing) return existing;
  const created: CompanyCache = { reports: {}, pages: {}, currentReportId: null, revision: state.revision ?? "" };
  state.companies[companyId] = created;
  return created;
}

function isCurrentGeneration(state: CompanyState, generation: number): boolean {
  return state.generation === generation;
}

const companySlice = createSlice({
  name: "company",
  initialState,
  reducers: {
    installCompanyBaseline(_state, action: PayloadAction<BaselinePayload>): CompanyState {
      return {
        generation: action.payload.generation,
        civilDate: action.payload.civilDate,
        dayStatus: null,
        revision: action.payload.revision,
        lastEventSeq: action.payload.seq,
        selectedCompanyId: null,
        companies: {},
      };
    },
    selectCompany(state, action: PayloadAction<string | null>) {
      state.selectedCompanyId = action.payload;
    },
    startCompanyQuery(state, action: PayloadAction<QueryPayload>) {
      if (!isCurrentGeneration(state, action.payload.generation)) return;
      companyCache(state, action.payload.companyId).pages[pageKey(action.payload.cursor)] = { kind: "loading" };
    },
    recordCompanyPage(state, action: PayloadAction<PagePayload>) {
      if (!isCurrentGeneration(state, action.payload.generation)) return;
      const cache = companyCache(state, action.payload.companyId);
      for (const report of action.payload.reports) cache.reports[report.id] ??= report;
      const reportIds = action.payload.reports.map((report) => report.id);
      cache.pages[pageKey(action.payload.cursor)] = reportIds.length === 0 && action.payload.nextCursor === null
        ? { kind: "empty" }
        : { kind: "ready", reportIds, nextCursor: action.payload.nextCursor };
      const firstReportId = reportIds[0];
      if (cache.currentReportId === null && firstReportId !== undefined) cache.currentReportId = firstReportId;
      cache.revision = state.revision ?? "";
    },
    recordCompanyReport(state, action: PayloadAction<ReportPayload>) {
      if (!isCurrentGeneration(state, action.payload.generation)) return;
      const cache = companyCache(state, action.payload.companyId);
      cache.reports[action.payload.report.id] ??= action.payload.report;
      cache.currentReportId ??= action.payload.report.id;
      cache.revision = state.revision ?? "";
    },
    recordCompanyQueryFailure(state, action: PayloadAction<FailurePayload>) {
      if (!isCurrentGeneration(state, action.payload.generation)) return;
      companyCache(state, action.payload.companyId).pages[pageKey(action.payload.cursor)] = {
        kind: "error", message: action.payload.message,
      };
    },
    unavailable(state, action: PayloadAction<FailurePayload>) {
      if (!isCurrentGeneration(state, action.payload.generation)) return;
      companyCache(state, action.payload.companyId).pages[pageKey(action.payload.cursor)] = {
        kind: "unavailable", message: action.payload.message,
      };
    },
    recordCivilDateAdvanced(state, action: PayloadAction<CivilDatePayload>) {
      if (!isCurrentGeneration(state, action.payload.generation) || action.payload.seq <= state.lastEventSeq) return;
      state.lastEventSeq = action.payload.seq;
      state.civilDate = action.payload.civilDate;
      state.dayStatus = action.payload.dayStatus;
    },
    invalidateCompanyForDisclosure(state, action: PayloadAction<DisclosurePayload>) {
      if (!isCurrentGeneration(state, action.payload.generation) || action.payload.seq <= state.lastEventSeq) return;
      state.lastEventSeq = action.payload.seq;
      const cache = state.companies[action.payload.companyId];
      if (!cache) return;
      cache.pages = {};
      cache.currentReportId = null;
    },
    advanceCompanyEventCoverage(state, action: PayloadAction<EventCoveragePayload>) {
      if (!isCurrentGeneration(state, action.payload.generation)) return;
      if (action.payload.toSeq > state.lastEventSeq) state.lastEventSeq = action.payload.toSeq;
    },
    reconcileCompanyPublicMetadata(state, action: PayloadAction<PublicMetadataPayload>) {
      if (!isCurrentGeneration(state, action.payload.generation)) return;
      if (action.payload.civilDate !== null) state.civilDate = action.payload.civilDate;
      if (action.payload.revision !== null) state.revision = action.payload.revision;
    },
  },
});

export const {
  installCompanyBaseline,
  selectCompany,
  startCompanyQuery,
  recordCompanyPage,
  recordCompanyReport,
  recordCompanyQueryFailure,
  unavailable,
  recordCivilDateAdvanced,
  invalidateCompanyForDisclosure,
  advanceCompanyEventCoverage,
  reconcileCompanyPublicMetadata,
} = companySlice.actions;
export const companyReducer = companySlice.reducer;
export type { CompanyCache, CompanyPage, CompanyState };
