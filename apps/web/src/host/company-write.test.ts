import assert from "node:assert/strict";
import test from "node:test";
import { CompanyWriteRejectedError, createCompanyWriteCommands, parseCompanyWriteFailure, type CompanyWriteCommand } from "./company-write.ts";
import { prepareCatalogResolver } from "./catalog-resolver.ts";

// synthetic 公共 wire fixture：只固定 Rust owner 已对齐的失败结构，不冒实际领域执行。
const proposalFailure = (classification = "BusinessCondition") => ({ kind: "PlayerProposal", data: {
  account: "18446744073709551615", company: 3, kind: "CashDividend", day: "2030-06-03", classification, reason: "玩家总持仓为零：原领域理由",
} });
const correctionFailure = (data: unknown) => ({ kind: "ReportCorrection", data });

test("Proposal 公共错误保留四分类、本人上下文和具体 reason", { timeout: 10000 }, () => {
  for (const classification of ["InvalidInput", "BusinessCondition", "UnsupportedOperation", "SystemState"]) {
    const value = proposalFailure(classification);
    assert.deepEqual(parseCompanyWriteFailure(value), value);
  }
});

test("Correction 严格逐 variant 保留公开字段；单位 variant 省略 data", { timeout: 10000 }, () => {
  const variants = [
    ...["StaleEpoch", "EmptyOperationId", "EmptyReason", "EmptyEntries", "RevisionExhausted"].map(kind => ({ kind })),
    { kind: "OperationConflict", data: { operation_id: "correct-1", completed: true } },
    { kind: "OperationConflict", data: { operation_id: "correct-1", completed: false } },
    { kind: "NotPending", data: { operation_id: "correct-1" } },
    { kind: "AlreadyCompleted", data: { operation_id: "correct-1" } },
    { kind: "Journal", data: { reason: "原 AccountingError 理由" } },
    { kind: "FutureEntry", data: { source_id: Number.MAX_SAFE_INTEGER, date: "2030-06-04", current: "2030-06-03" } },
  ];
  for (const variant of variants) {
    const value = correctionFailure(variant);
    assert.deepEqual(parseCompanyWriteFailure(value), value);
  }
});

test("失败结构拒未知、缺字段与四分类静默补值，不把 Step fatal 当普通业务拒绝", { timeout: 10000 }, () => {
  const valid = proposalFailure();
  for (const key of Object.keys(valid.data)) {
    const data: Record<string, unknown> = { ...valid.data };
    delete data[key];
    assert.throws(() => parseCompanyWriteFailure({ ...valid, data }), new RegExp(key));
  }
  for (const value of [
    { ...valid, extra: true }, { ...valid, data: { ...valid.data, classification: "Other" } },
    { ...valid, data: { ...valid.data, account: 0 } }, { ...valid, data: { ...valid.data, company: -0 } },
    { ...valid, data: { ...valid.data, reason: "" } }, { ...valid, data: { ...valid.data, day: "2030-02-30" } },
    { ...valid, data: { ...valid.data, cause: "私有 Rust cause 不入 DTO" } },
    correctionFailure({ kind: "EmptyEntries", data: {} }), correctionFailure({ kind: "Step" }),
    correctionFailure({ kind: "NotPending", data: {} }),
    correctionFailure({ kind: "OperationConflict", data: { operation_id: "correct-1", completed: "true" } }),
    correctionFailure({ kind: "FutureEntry", data: { source_id: Number.MAX_SAFE_INTEGER + 1, date: "2030-06-04", current: "2030-06-03" } }),
    { code: "STEP_FATAL", message: "真正 fatal 走既有管道", context: {} },
  ]) assert.throws(() => parseCompanyWriteFailure(value));
});

test("typed 拒绝 Error 的 cause 保整个公开 failure，四分类不等于 fatal 推断", { timeout: 10000 }, () => {
  const failure = parseCompanyWriteFailure(proposalFailure("SystemState"));
  const error = new CompanyWriteRejectedError(failure);
  assert.equal(error.name, "CompanyWriteRejectedError");
  assert.equal(error.failure, failure);
  assert.equal(error.cause, failure);
  assert.match(error.message, /玩家总持仓为零/);
});

const identity = prepareCatalogResolver([{ id: 8, code: "600101" }], [{ id: 3, code: "实际发行人", name: "公司", listed_stock: 8 }]);
const proposal = () => ({ CashDividend: { company: 3, gross_per_share: "5" } });
const request = () => ({ operation_id: "correct-1", company: 3, supersedes: 7, entries: [{ source: 900000001, date: "2030-06-03", kind: "CashRevenue" as const, cash_flow: "Operating" as const, lines: [{ account: "1002", side: "Debit" as const, amount: "10.00" }, { account: "6001", side: "Credit" as const, amount: "10.00" }] }], reason: "synthetic transport fixture" });
const receipt = () => ({ account: "0", company: 3, kind: "CashDividend", event_id: "synthetic-approved", approved_on: "2030-06-03" });

test("三命令 factory 只透传实际 transport 并解包 receipt/ack，不执行本地业务", { timeout: 10000 }, async () => {
  const sent: CompanyWriteCommand[] = [];
  const commands = createCompanyWriteCommands(async command => {
    sent.push(command);
    return command.kind === "ProposePlayerCompanyAction" ? { receipt: receipt() } : { operation_id: "correct-1" };
  }, () => identity, "0");
  assert.deepEqual(await commands.proposePlayerCompanyAction(proposal()), receipt());
  assert.deepEqual(await commands.enqueueReportCorrection(request()), { operation_id: "correct-1" });
  assert.deepEqual(await commands.cancelReportCorrection("correct-1"), { operation_id: "correct-1" });
  assert.deepEqual(sent, [{ kind: "ProposePlayerCompanyAction", proposal: proposal() }, { kind: "EnqueueReportCorrection", request: request() }, { kind: "CancelReportCorrection", operation_id: "correct-1" }]);
});

test("三命令拒绝结构多余权限字段、未知目录与非法数值，发送前失败", { timeout: 10000 }, async () => {
  let sent = 0;
  const commands = createCompanyWriteCommands(async () => { sent++; throw new Error("不能发送"); }, () => identity, "0");
  await assert.rejects(commands.proposePlayerCompanyAction({ CashDividend: { ...proposal().CashDividend, account: "0" } } as never), /account/);
  await assert.rejects(commands.proposePlayerCompanyAction({ CashDividend: { ...proposal().CashDividend, company: 8 } }), /CompanyId/);
  await assert.rejects(commands.enqueueReportCorrection({ ...request(), epoch: "伪造" } as never), /epoch/);
  await assert.rejects(commands.enqueueReportCorrection({ ...request(), company: 8 }), /CompanyId/);
  await assert.rejects(commands.cancelReportCorrection(7 as never), /operation_id/);
  assert.equal(sent, 0);
});

test("成功 receipt/ack 严格核本人、公司、种类、operation_id，拒绝额外字段", { timeout: 10000 }, async () => {
  for (const result of [
    { receipt: { ...receipt(), account: "1" } }, { receipt: { ...receipt(), company: 8 } },
    { receipt: { ...receipt(), kind: "ShareSplit" } }, { receipt: { ...receipt(), approved_on: "2030-02-30" } },
    { receipt: { ...receipt(), event_id: "" } }, { receipt: { ...receipt(), completed: true } },
    { generation: "1", receipt: receipt() },
  ]) {
    const commands = createCompanyWriteCommands(async () => result, () => identity, "0");
    await assert.rejects(commands.proposePlayerCompanyAction(proposal()));
  }
  for (const result of [{ operation_id: "other" }, { operation_id: "correct-1", completed: true }, {}]) {
    const commands = createCompanyWriteCommands(async () => result, () => identity, "0");
    await assert.rejects(commands.enqueueReportCorrection(request()));
    await assert.rejects(commands.cancelReportCorrection("correct-1"));
  }
});

test("typed 拒绝同样核 family/本人/公司/种类/operation_id；未知错误原样上抛", { timeout: 10000 }, async () => {
  const matching = new CompanyWriteRejectedError(parseCompanyWriteFailure({ ...proposalFailure(), data: { ...proposalFailure().data, account: "0" } }));
  const rejected = createCompanyWriteCommands(async () => { throw matching; }, () => identity, "0");
  await assert.rejects(rejected.proposePlayerCompanyAction(proposal()), error => error === matching);
  for (const failure of [
    proposalFailure(), { ...proposalFailure(), data: { ...proposalFailure().data, account: "0", company: 8 } },
    { ...proposalFailure(), data: { ...proposalFailure().data, account: "0", kind: "ShareSplit" } },
    correctionFailure({ kind: "NotPending", data: { operation_id: "correct-1" } }),
  ]) {
    const original = new CompanyWriteRejectedError(parseCompanyWriteFailure(failure));
    const commands = createCompanyWriteCommands(async () => { throw original; }, () => identity, "0");
    await assert.rejects(commands.proposePlayerCompanyAction(proposal()), error => error instanceof Error && error !== original && error.cause === original);
  }
  const original = new CompanyWriteRejectedError(parseCompanyWriteFailure(correctionFailure({ kind: "NotPending", data: { operation_id: "other" } })));
  const commands = createCompanyWriteCommands(async () => { throw original; }, () => identity, "0");
  await assert.rejects(commands.cancelReportCorrection("correct-1"), error => error instanceof Error && error.cause === original);
  const network = new Error("连接中断，结果未知");
  const unknown = createCompanyWriteCommands(async () => { throw network; }, () => identity, "0");
  await assert.rejects(unknown.cancelReportCorrection("correct-1"), error => error === network);
});

test("发送时快照请求，调用方后续改值不能篡改回执匹配目标", { timeout: 10000 }, async () => {
  let resolve!: (value: unknown) => void;
  let sent!: CompanyWriteCommand;
  const commands = createCompanyWriteCommands(command => { sent = command; return new Promise(complete => { resolve = complete; }); }, () => identity, "0");
  const input = proposal();
  const pending = commands.proposePlayerCompanyAction(input);
  input.CashDividend.company = 8;
  resolve({ receipt: receipt() });
  assert.deepEqual(await pending, receipt());
  assert.deepEqual(sent, { kind: "ProposePlayerCompanyAction", proposal: proposal() });
});
