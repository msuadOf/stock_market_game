import type { CompanyErrorClass } from "../types/generated/CompanyErrorClass.ts";
import { parsePlayerCompanyProposal, type PlayerCompanyProposalInput } from "./player-proposals.ts";
import { parseCompanyReportCorrection, type CompanyReportCorrectionInput } from "./report-corrections.ts";
import type { CatalogResolver } from "./catalog-resolver.ts";
import { boundedInteger, companyCivilDate, nonempty, ownerAccount } from "./company-contract-views.ts";
import { boolean, exact, integer, oneOf, record, string } from "../save/schema/primitives.ts";

const PROPOSAL_KINDS = ["CashDividend", "StockDistribution", "RightsOffering", "SecondaryOffering", "IssuerRepurchase", "ShareSplit"] as const;
type PlayerProposalKind = typeof PROPOSAL_KINDS[number];

export interface PlayerCompanyCommandReceiptView {
  readonly account: string;
  readonly company: number;
  readonly kind: PlayerProposalKind;
  readonly event_id: string;
  readonly approved_on: string;
}

/** 只确认命令实际 Ok；completed 的 exact 重试也可受理，不证明 pending 或公布。 */
export interface ReportCorrectionCommandAck { readonly operation_id: string }

export type CompanyWriteCommand =
  | { readonly kind: "ProposePlayerCompanyAction"; readonly proposal: PlayerCompanyProposalInput }
  | { readonly kind: "EnqueueReportCorrection"; readonly request: CompanyReportCorrectionInput }
  | { readonly kind: "CancelReportCorrection"; readonly operation_id: string };

export interface CompanyWriteCommands {
  proposePlayerCompanyAction(proposal: PlayerCompanyProposalInput): Promise<PlayerCompanyCommandReceiptView>;
  enqueueReportCorrection(request: CompanyReportCorrectionInput): Promise<ReportCorrectionCommandAck>;
  cancelReportCorrection(operationId: string): Promise<ReportCorrectionCommandAck>;
}

type ReportCorrectionFailure =
  | { readonly kind: "StaleEpoch" | "EmptyOperationId" | "EmptyReason" | "EmptyEntries" | "RevisionExhausted" }
  | { readonly kind: "OperationConflict"; readonly data: { readonly operation_id: string; readonly completed: boolean } }
  | { readonly kind: "NotPending" | "AlreadyCompleted"; readonly data: { readonly operation_id: string } }
  | { readonly kind: "Journal"; readonly data: { readonly reason: string } }
  | { readonly kind: "FutureEntry"; readonly data: { readonly source_id: number; readonly date: string; readonly current: string } };

export type CompanyWriteFailure =
  | { readonly kind: "PlayerProposal"; readonly data: {
    readonly account: string; readonly company: number; readonly kind: PlayerProposalKind; readonly day: string;
    readonly classification: CompanyErrorClass; readonly reason: string;
  } }
  | { readonly kind: "ReportCorrection"; readonly data: ReportCorrectionFailure };

function companyId(value: unknown, path: string): number { return boundedInteger(value, path, 0, 4294967295); }

/** 只消费 Rust 公开的 typed failure；不解析 Display、不访问私有 source，不按分类猜 fatal。 */
export function parseCompanyWriteFailure(value: unknown): CompanyWriteFailure {
  const path = "CompanyWriteFailure";
  const wrapper = record(value, path);
  exact(wrapper, ["kind", "data"], path);
  const family = oneOf(wrapper.kind, `${path}.kind`, ["PlayerProposal", "ReportCorrection"] as const);
  const data = record(wrapper.data, `${path}.data`);
  if (family === "PlayerProposal") {
    exact(data, ["account", "company", "kind", "day", "classification", "reason"], `${path}.data`);
    return { kind: family, data: {
      account: ownerAccount(data.account, `${path}.data.account`), company: companyId(data.company, `${path}.data.company`),
      kind: oneOf(data.kind, `${path}.data.kind`, PROPOSAL_KINDS), day: companyCivilDate(data.day, `${path}.data.day`),
      classification: oneOf(data.classification, `${path}.data.classification`, ["InvalidInput", "BusinessCondition", "UnsupportedOperation", "SystemState"] as const),
      reason: nonempty(data.reason, `${path}.data.reason`),
    } };
  }
  const kind = oneOf(data.kind, `${path}.data.kind`, ["StaleEpoch", "EmptyOperationId", "EmptyReason", "EmptyEntries", "RevisionExhausted", "OperationConflict", "NotPending", "AlreadyCompleted", "Journal", "FutureEntry"] as const);
  if (kind === "StaleEpoch" || kind === "EmptyOperationId" || kind === "EmptyReason" || kind === "EmptyEntries" || kind === "RevisionExhausted") {
    exact(data, ["kind"], `${path}.data`);
    return { kind: family, data: { kind } };
  }
  exact(data, ["kind", "data"], `${path}.data`);
  const fields = record(data.data, `${path}.data.data`);
  if (kind === "OperationConflict") {
    exact(fields, ["operation_id", "completed"], `${path}.data.data`);
    return { kind: family, data: { kind, data: { operation_id: string(fields.operation_id, `${path}.data.data.operation_id`), completed: boolean(fields.completed, `${path}.data.data.completed`) } } };
  }
  if (kind === "NotPending" || kind === "AlreadyCompleted") {
    exact(fields, ["operation_id"], `${path}.data.data`);
    return { kind: family, data: { kind, data: { operation_id: string(fields.operation_id, `${path}.data.data.operation_id`) } } };
  }
  if (kind === "Journal") {
    exact(fields, ["reason"], `${path}.data.data`);
    return { kind: family, data: { kind, data: { reason: nonempty(fields.reason, `${path}.data.data.reason`) } } };
  }
  exact(fields, ["source_id", "date", "current"], `${path}.data.data`);
  return { kind: family, data: { kind, data: {
    source_id: integer(fields.source_id, `${path}.data.data.source_id`, 0),
    date: companyCivilDate(fields.date, `${path}.data.data.date`), current: companyCivilDate(fields.current, `${path}.data.data.current`),
  } } };
}

export class CompanyWriteRejectedError extends Error {
  readonly failure: CompanyWriteFailure;

  constructor(failure: CompanyWriteFailure) {
    super(`公司写命令被拒绝：${failure.kind === "PlayerProposal" ? failure.data.reason : JSON.stringify(failure.data)}`, { cause: failure });
    this.name = "CompanyWriteRejectedError";
    this.failure = failure;
  }
}

function proposalIdentity(proposal: PlayerCompanyProposalInput): { readonly company: number; readonly kind: PlayerProposalKind } {
  const kind = Object.keys(proposal)[0] as PlayerProposalKind;
  const payload = Object.values(proposal)[0] as { readonly company: number };
  return { kind, company: payload.company };
}

function requireEqual(actual: unknown, expected: unknown, path: string): void {
  if (actual !== expected) throw new Error(`${path} 与本次公司写命令不一致`);
}

function assertFailureMatches(command: CompanyWriteCommand, value: CompanyWriteFailure, expectedOwner?: string): void {
  if (command.kind === "ProposePlayerCompanyAction") {
    if (value.kind !== "PlayerProposal") throw new Error("公司提案失败 family 与命令不一致");
    const expected = proposalIdentity(command.proposal);
    requireEqual(value.data.company, expected.company, "failure.company");
    requireEqual(value.data.kind, expected.kind, "failure.kind");
    if (expectedOwner !== undefined) requireEqual(value.data.account, expectedOwner, "failure.account");
    return;
  }
  if (value.kind !== "ReportCorrection") throw new Error("公司更正失败 family 与命令不一致");
  const variant = value.data;
  if (variant.kind === "NotPending" || variant.kind === "AlreadyCompleted" || variant.kind === "OperationConflict") {
    const operationId = command.kind === "EnqueueReportCorrection" ? command.request.operation_id : command.operation_id;
    requireEqual(variant.data.operation_id, operationId, "failure.operation_id");
  }
}

/** 共用结构与身份校验；transport 必须调用真实 Host，不在这里批准、入队或取消。 */
export function createCompanyWriteCommands(
  transport: (command: CompanyWriteCommand) => Promise<unknown>,
  identity: () => CatalogResolver,
  expectedOwner?: string,
): CompanyWriteCommands {
  if (expectedOwner !== undefined) ownerAccount(expectedOwner, "company_write.expectedOwner");
  async function send(command: CompanyWriteCommand): Promise<unknown> {
    try { return await transport(command); }
    catch (error) {
      if (error instanceof CompanyWriteRejectedError) {
        try { assertFailureMatches(command, parseCompanyWriteFailure(error.failure), expectedOwner); }
        catch (decoderError) {
          throw new AggregateError([decoderError], "公司写命令拒绝响应不符合本次请求的协议或身份", { cause: error });
        }
      }
      throw error;
    }
  }
  async function correction(command: Extract<CompanyWriteCommand, { kind: "EnqueueReportCorrection" | "CancelReportCorrection" }>): Promise<ReportCorrectionCommandAck> {
    const raw = record(await send(command), "company_write.ack");
    exact(raw, ["operation_id"], "company_write.ack");
    const operationId = string(raw.operation_id, "company_write.ack.operation_id");
    requireEqual(operationId, command.kind === "EnqueueReportCorrection" ? command.request.operation_id : command.operation_id, "ack.operation_id");
    return { operation_id: operationId };
  }
  return {
    async proposePlayerCompanyAction(input) {
      const proposal = structuredClone(parsePlayerCompanyProposal(input));
      const expected = proposalIdentity(proposal);
      identity().companyCode(expected.company, "company_write.proposal.CompanyId");
      const result = record(await send({ kind: "ProposePlayerCompanyAction", proposal }), "company_write.proposal");
      exact(result, ["receipt"], "company_write.proposal");
      const raw = record(result.receipt, "company_write.receipt");
      exact(raw, ["account", "company", "kind", "event_id", "approved_on"], "company_write.receipt");
      const receipt: PlayerCompanyCommandReceiptView = {
        account: ownerAccount(raw.account, "company_write.receipt.account"), company: companyId(raw.company, "company_write.receipt.company"),
        kind: oneOf(raw.kind, "company_write.receipt.kind", PROPOSAL_KINDS), event_id: nonempty(raw.event_id, "company_write.receipt.event_id"),
        approved_on: companyCivilDate(raw.approved_on, "company_write.receipt.approved_on"),
      };
      requireEqual(receipt.company, expected.company, "receipt.company");
      requireEqual(receipt.kind, expected.kind, "receipt.kind");
      if (expectedOwner !== undefined) requireEqual(receipt.account, expectedOwner, "receipt.account");
      return receipt;
    },
    async enqueueReportCorrection(input) {
      const request = parseCompanyReportCorrection(input);
      identity().companyCode(request.company, "company_write.correction.CompanyId");
      return correction({ kind: "EnqueueReportCorrection", request });
    },
    async cancelReportCorrection(operationId) {
      return correction({ kind: "CancelReportCorrection", operation_id: string(operationId, "company_write.cancel.operation_id") });
    },
  };
}
