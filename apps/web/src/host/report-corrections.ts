import { parseCompanyReportCorrection, parseReportCorrections, type CompanyReportCorrection, type CompletedReportCorrection } from "../save/schema/company/report-corrections.ts";
import { exact, record, text, values } from "./protocol/guards.ts";
import type { HostFailure } from "./host-update.ts";

export type { CompanyReportCorrection, CompletedReportCorrection };

export interface ReportCorrectionStatus {
  readonly pending: readonly CompanyReportCorrection[];
  readonly completed: Readonly<Record<string, CompletedReportCorrection>>;
}

export function reportCorrectionFailureGeneration(failure: HostFailure): string {
  const context = record(failure.context, "财报更正失败 context");
  const generation = text(context.generation, "财报更正失败 context.generation");
  if (!/^(0|[1-9]\d*)$/.test(generation)) throw new Error("财报更正失败 context.generation 必须是规范非负整数字符串");
  return generation;
}

export function parseReportCorrectionStatus(value: unknown): ReportCorrectionStatus {
  const source = record(value, "ReportCorrectionStatus");
  exact(source, ["pending", "completed"], "ReportCorrectionStatus");
  const completed = parseReportCorrections(source.completed, "ReportCorrectionStatus.completed");
  const identities = new Set(Object.keys(completed));
  const pending = values(source.pending, "ReportCorrectionStatus.pending").map((value, index) => {
    const request = parseCompanyReportCorrection(value, `ReportCorrectionStatus.pending[${index}]`);
    if (identities.has(request.operation_id)) throw new Error("ReportCorrectionStatus operation_id 重复或同时待办与完成");
    identities.add(request.operation_id);
    return request;
  });
  return { pending, completed };
}

export function parseReportCorrectionResponse(value: unknown, generation: string, mutation: true): null;
export function parseReportCorrectionResponse(value: unknown, generation: string, mutation: false): ReportCorrectionStatus;
export function parseReportCorrectionResponse(value: unknown, generation: string, mutation: boolean): ReportCorrectionStatus | null {
  const source = record(value, "report correction response");
  exact(source, ["generation", "value"], "report correction response");
  const actual = text(source.generation, "report correction response.generation");
  if (!/^(0|[1-9]\d*)$/.test(actual) || actual !== generation) throw new Error("财报更正响应属于已过期或不合法 generation");
  if (mutation) {
    if (source.value !== null) throw new Error("财报更正 mutation 确认 value 必须是 null；入队不等于公开成功");
    return null;
  }
  return parseReportCorrectionStatus(source.value);
}
