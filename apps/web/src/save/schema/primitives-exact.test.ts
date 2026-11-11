import assert from "node:assert/strict";
import test from "node:test";
import { exact } from "./primitives.ts";

test("exact 保持缺失优先、首个未知字段及继承字段语义", { timeout: 10000 }, () => {
  assert.throws(() => exact({ extra: 1 }, ["first", "second"], "x"), /x.first.*必填/);
  assert.throws(() => exact({ known: 1, first: 2, second: 3 }, ["known"], "x"), /x.first.*允许/);
  assert.throws(() => exact(Object.create({ inherited: 1 }), ["inherited"], "x"), /x.inherited.*必填/);
  const keys = Array.from({ length: 100 }, (_, i) => String(i));
  const full = Object.fromEntries(keys.map(key => [key, 1]));
  assert.doesNotThrow(() => exact(full, keys, "x"));
  const { "0": omitted, ...missing } = full; void omitted;
  assert.throws(() => exact({ ...missing, extra: 1 }, keys, "x"), /x.0.*必填/);
  assert.throws(() => exact({ ...full, extra: 1, later: 2 }, keys, "x"), /x.extra.*允许/);
});

test("常见短键表复用成员索引，避免每个对象重复分配 Set", { timeout: 10000 }, () => {
  const original = globalThis.Set;
  let constructions = 0;
  globalThis.Set = class<T> extends original<T> { constructor(values?: readonly T[] | null) { super(values); constructions += 1; } } as typeof Set;
  try {
    for (let i = 0; i < 20; i += 1) exact({ a: 1, b: 2 }, ["a", "b"], "x");
    assert.equal(constructions, 0);
  } finally { globalThis.Set = original; }
});

test("map 保留原型同名键为普通自有字段", async () => {
  const { map } = await import("./primitives.ts");
  const source = JSON.parse('{"__proto__":1,"constructor":2,"normal":3}');
  const parsed = map(source, "测试", () => {}, (value) => value);
  assert.deepEqual(parsed, source);
  assert.equal(Object.getPrototypeOf(parsed), Object.prototype);
  assert.ok(Object.hasOwn(parsed, "__proto__"));
});
