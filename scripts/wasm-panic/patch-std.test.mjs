import assert from "node:assert/strict";
import test from "node:test";
import { patchPanicSource } from "./patch-std.mjs";

const fixture = `fn panic_with_hook(
    payload: &mut dyn PanicPayload,
    location: &'static Location<'static>,
    can_unwind: bool,
    force_no_backtrace: bool,
) -> ! {
    let must_abort = panic_count::increase(true);
}
`;
test("共同 panic 入口在标准库处理前调用诊断入口", { timeout: 10000 }, () => {
  const patched = patchPanicSource(fixture);
  assert.match(patched, /panic_with_hook\([^]*?wasm_tombstone_early[^]*?panic_count::increase/);
  assert.match(patched, /payload\.as_str\(\)/);
  assert.match(patched, /fn wasm_tombstone_std_marker\(\)/);
  assert.equal((patched.match(/intrinsics::abort\(\)/g) ?? []).length, 1);
});
test("缺失、重复或已打补丁的入口必须显式失败", { timeout: 10000 }, () => {
  assert.throws(() => patchPanicSource(""), /入口/);
  assert.throws(() => patchPanicSource(fixture + fixture), /入口/);
  assert.throws(() => patchPanicSource(patchPanicSource(fixture)), /入口/);
});
