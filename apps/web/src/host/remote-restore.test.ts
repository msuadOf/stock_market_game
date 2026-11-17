import test from "node:test";
import assert from "node:assert/strict";
import { remoteRestoreJson, RemoteRestoreFailure, parseRestoreCommit } from "./remote-restore.ts";
test("typed读档区分提交前拒绝和未知，提交回执不得猜测generation", { timeout: 10000 }, async () => {
  for (const outcome of ["rejected", "unknown"] as const) {
    await assert.rejects(remoteRestoreJson(async () => new Response('{"code":"FAILED","message":"明确失败"}',
      { status: 400, headers: { "x-session-restore-outcome": outcome } }), "/load", {}),
      error => error instanceof RemoteRestoreFailure && error.outcome === outcome);
  }
  assert.equal(parseRestoreCommit({outcome:"committed",replacement:{previous_generation:"1",generation:"2"},snapshot:{}}, "1"), "2");
  assert.throws(() => parseRestoreCommit({outcome:"committed",replacement:{previous_generation:"0",generation:"2"},snapshot:{}}, "1"));
  assert.throws(() => parseRestoreCommit({outcome:"committed",replacement:{previous_generation:"0",generation:"1"},snapshot:{}}, "1"));
});

test("并发controller提交的合法回执保持known commit，不被本地落后或领先基线改为unknown", { timeout: 10000 }, () => {
  const receipt = {outcome:"committed",replacement:{previous_generation:"5",generation:"6"},snapshot:{}};
  assert.equal(parseRestoreCommit(receipt,"1"),"6");
  assert.equal(parseRestoreCommit(receipt,"8"),"6");
});
