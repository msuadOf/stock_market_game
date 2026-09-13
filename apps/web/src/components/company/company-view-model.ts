import type { CompanyPage } from "../../store/company-slice.ts";
import type { PublicReportSummary } from "../../types/engine.ts";

type CompanyReports = {
  readonly reports: Record<string, PublicReportSummary>;
  readonly pages: Record<string, CompanyPage>;
};

export function visibleReports(cache: CompanyReports | undefined): { readonly reports: readonly PublicReportSummary[]; readonly nextCursor: string | null } {
  const ids = new Set<string>();
  const reports = cache?.reports ?? {};
  let pageKey = "root";
  let nextCursor: string | null = null;
  while (true) {
    const page = cache?.pages[pageKey];
    if (page?.kind !== "ready") return { reports: [...ids].flatMap((id) => reports[id] === undefined ? [] : [reports[id]]), nextCursor };
    for (const id of page.reportIds) ids.add(id);
    nextCursor = page.nextCursor;
    if (nextCursor === null) return { reports: [...ids].flatMap((id) => reports[id] === undefined ? [] : [reports[id]]), nextCursor: null };
    pageKey = nextCursor;
  }
}
