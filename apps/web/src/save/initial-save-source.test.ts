import assert from "node:assert/strict";
import test from "node:test";
import * as replacement from "./session-replacement.ts";

type Source<Value> = {
  read(load: () => Promise<Value | null>): Promise<Value | null>;
  complete(): void;
  select(value: Value): void;
  reset(): void;
};

function source<Value>(): Source<Value> {
  const Constructor = Reflect.get(replacement, "InitialSaveSource");
  assert.equal(typeof Constructor, "function", "startup requires a one-time archive source");
  return new (Constructor as new () => Source<Value>)();
}

test("initial reads share one disk request and completion releases the archive", async () => {
  const initial = source<number>();
  let reads = 0;
  const load = async () => { reads += 1; return 7; };
  const first = initial.read(load);
  const second = initial.read(load);
  assert.equal(first, second);
  assert.deepEqual(await Promise.all([first, second]), [7, 7]);
  assert.equal(reads, 1);
  initial.complete();
  assert.equal(await initial.read(load), null);
  assert.equal(reads, 1);
});

test("a missing archive starts once without polling storage during gameplay", async () => {
  const initial = source<number>();
  let reads = 0;
  const load = async () => { reads += 1; return null; };
  assert.equal(await initial.read(load), null);
  initial.complete();
  for (let frame = 0; frame < 4; frame += 1) assert.equal(await initial.read(load), null);
  assert.equal(reads, 1);
});

test("failed initial reads retain their exact error and do not silently retry or start a new game", async () => {
  const initial = source<number>();
  const failure = new Error("archive decompression failed");
  let reads = 0;
  const load = async () => { reads += 1; throw failure; };
  await assert.rejects(initial.read(load), (error) => error === failure);
  await assert.rejects(initial.read(load), (error) => error === failure);
  assert.equal(reads, 1);
});

test("synchronous storage errors become one cached rejected request", async () => {
  const initial = source<number>();
  const failure = new Error("storage access denied");
  let reads = 0;
  const load = () => { reads += 1; throw failure; };
  const pending = initial.read(load);
  await assert.rejects(pending, (error) => error === failure);
  await assert.rejects(initial.read(load), (error) => error === failure);
  assert.equal(reads, 1);
});

test("explicit new game does not reload the previous archive", async () => {
  const initial = source<number>();
  let reads = 0;
  initial.reset();
  assert.equal(await initial.read(async () => { reads += 1; return 7; }), null);
  assert.equal(reads, 0);
});

test("explicit archive selection replaces a pending initial read without rereading storage", async () => {
  const initial = source<number>();
  let finish: ((value: number) => void) | undefined;
  const old = initial.read(() => new Promise<number>((resolve) => { finish = resolve; }));
  await Promise.resolve();
  initial.select(9);
  let reads = 0;
  assert.equal(await initial.read(async () => { reads += 1; return 7; }), 9);
  assert.ok(finish);
  finish(7);
  assert.equal(await old, 7);
  assert.equal(await initial.read(async () => { reads += 1; return 7; }), 9);
  assert.equal(reads, 0);
  initial.complete();
  assert.equal(await initial.read(async () => { reads += 1; return 7; }), null);
  assert.equal(reads, 0);
});
