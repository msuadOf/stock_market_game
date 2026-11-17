import assert from "node:assert/strict";
import test from "node:test";
import { readAuthorityContractSnapshot } from "./wasm-authority-contract.ts";

test("验证snapshot按真实serde嵌套Map读取，保留key与诊断", { timeout: 10000 }, () => {
  const key = new Map<string, unknown>([["generation", "1"], ["writerEpoch", "2"], ["seq", "3"]]);
  const raw = new Map<string, unknown>([
    ["id", "manual"], ["generation", "1"], ["day", 1], ["tick", "3"], ["seq", "3"], ["civilReady", true],
    ["archive", new Map<string, unknown>([["queued", [key]], ["durable", null], ["blocked", false]])],
    ["observer", new Map<string, unknown>([["lease", "7"], ["accepted", "0"]])],
  ]);
  const row = readAuthorityContractSnapshot({ market_runner_contract_snapshot: (handle, id) => {
    assert.equal(handle, 5); assert.equal(id, "manual"); return raw;
  } }, 5, "manual");
  assert.equal(row.id, "manual"); assert.equal(row.civilReady, true);
  assert.equal(row.archive.queued[0]!.writerEpoch, "2");
  assert.equal(row.observer.lease, "7");
  assert.ok(raw.get("archive") instanceof Map, "标准化不能修改Rust返回的原始值");
});

test("验证snapshot拒绝错误根值与不匹配步骤", { timeout: 10000 }, () => {
  assert.throws(() => readAuthorityContractSnapshot({ market_runner_contract_snapshot: () => null }, 1, "manual"), /snapshot/);
  assert.throws(() => readAuthorityContractSnapshot({ market_runner_contract_snapshot: () => ({ id: "old" }) }, 1, "manual"), /snapshot/);
});

test("验证发布Map正确区分有序恢复包和一次terminal", { timeout: 10000 }, async () => {
  const { normalizeAuthorityContractPacket } = await import("./wasm-authority-contract.ts");
  const terminal = normalizeAuthorityContractPacket(new Map<string, unknown>([["kind", "failure"], ["failure", new Map([["code", "STEP_FATAL"]])]]));
  assert.equal(terminal.kind, "failure");
  assert.equal(Object.hasOwn(terminal, "publicationOrdinal"), false);
  const recovery = normalizeAuthorityContractPacket(new Map<string, unknown>([["kind", "failure"], ["publicationOrdinal", "4"], ["failure", new Map([["code", "DAY_END_ARCHIVE_FAILED"]])]]));
  assert.equal(recovery.publicationOrdinal, "4");
  assert.throws(() => normalizeAuthorityContractPacket({ kind: "unknown" }), /发布包/);
});

test("慢观察者只确认发送，前缀排空场景在实际接收后独立接纳", { timeout: 10000 }, async () => {
  const { confirmAuthorityContractPacket } = await import("./wasm-authority-contract.ts");
  const calls: string[] = [];
  const bindings = {
    market_publisher_flushed: (_handle: number, _lease: bigint, ordinal: bigint) => { calls.push(`flushed:${ordinal}`); },
    market_publisher_ack: (_handle: number, _lease: bigint, ordinal: bigint) => { calls.push(`accepted:${ordinal}`); },
  };
  confirmAuthorityContractPacket(bindings, 1, 2n, { publicationOrdinal: "3" }, false);
  assert.deepEqual(calls, ["flushed:3"]);
  confirmAuthorityContractPacket(bindings, 1, 2n, { publicationOrdinal: "4" }, true);
  assert.deepEqual(calls, ["flushed:3", "flushed:4", "accepted:4"]);
  confirmAuthorityContractPacket(bindings, 1, 2n, { kind: "failure" }, true);
  assert.equal(calls.length, 3, "terminal不冒充有序应用ACK");
});
