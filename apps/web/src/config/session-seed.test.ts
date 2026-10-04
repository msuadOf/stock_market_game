import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { createNewSessionSeed, type SeedEntropy } from "./session-seed.ts";

function entropyWords(high: number, low: number): SeedEntropy {
  return {
    getRandomValues(array) {
      assert.ok(array instanceof Uint32Array);
      assert.equal(array.length, 2);
      array.set([high, low]);
      return array;
    },
  };
}

describe("新局 seed 熵边界", { timeout: 10000 }, () => {
  it("保留完整 u64 精度并允许固定熵注入", () => {
    assert.equal(createNewSessionSeed(entropyWords(0xffffffff, 0xffffffff)), 18446744073709551615n);
    assert.equal(createNewSessionSeed(entropyWords(0x12345678, 0x90abcdef)), 0x1234567890abcdefn);
    assert.equal(createNewSessionSeed(entropyWords(0, 0)), 0n);
  });

  it("每次新局重新读取熵，不复用固定 seed", () => {
    let reads = 0;
    const entropy: SeedEntropy = {
      getRandomValues(array) {
        assert.ok(array instanceof Uint32Array);
        array.set([0, ++reads]);
        return array;
      },
    };
    assert.equal(createNewSessionSeed(entropy), 1n);
    assert.equal(createNewSessionSeed(entropy), 2n);
    assert.equal(reads, 2);
  });

  it("熵能力缺失或调用失败时显式报错，不降级到固定 seed", () => {
    assert.throws(() => createNewSessionSeed(null), /新局 seed.*getRandomValues/);
    assert.throws(() => createNewSessionSeed({} as SeedEntropy), /新局 seed.*getRandomValues/);
    const cause = new Error("权限拒绝");
    try {
      createNewSessionSeed({ getRandomValues() { throw cause; } });
      assert.fail("熵调用失败必须抛错");
    } catch (error) {
      assert.ok(error instanceof Error);
      assert.match(error.message, /新局 seed.*权限拒绝/);
      assert.equal(error.cause, cause);
    }
  });
});
