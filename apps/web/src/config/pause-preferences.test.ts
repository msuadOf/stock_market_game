import assert from "node:assert/strict";
import test from "node:test";
import { defaultPausePreferences, loadPausePreferences, savePausePreferences } from "./pause-preferences.ts";

class MemoryStorage {
  value: string | null = null;
  getItem(): string | null { return this.value; }
  setItem(_key: string, value: string): void { this.value = value; }
}

test("Given no persisted pause preferences, when loaded, then both barriers default to false", () => {
  assert.deepEqual(loadPausePreferences(new MemoryStorage()), defaultPausePreferences());
});

test("Given valid pause preferences, when persisted and loaded, then both flags round trip", () => {
  const storage = new MemoryStorage();
  savePausePreferences(storage, { pause_after_close: true, pause_before_open: false });
  assert.deepEqual(loadPausePreferences(storage), { pause_after_close: true, pause_before_open: false });
});

test("Given malformed persisted pause preferences, when loaded, then it fails visibly", () => {
  const storage = new MemoryStorage();
  storage.value = '{"pause_after_close":true}';
  assert.throws(() => loadPausePreferences(storage), /两个布尔值/);
});
