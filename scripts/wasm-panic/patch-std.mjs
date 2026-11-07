const signature = `fn panic_with_hook(
    payload: &mut dyn PanicPayload,
    location: &'static Location<'static>,
    can_unwind: bool,
    force_no_backtrace: bool,
) -> ! {`;
const anchor = `${signature}\n    let must_abort = panic_count::increase(true);`;
const marker = "wasm_tombstone_early";
const replacement = `${signature}
    #[cfg(target_arch = "wasm32")]
    {
        unsafe extern "C" { fn wasm_tombstone_early(ptr: *const u8, len: usize, has_message: bool); }
        // panic_any 与动态格式可能没有免分配的字符串；不得推测其内容。
        let message = payload.as_str();
        let bytes = message.unwrap_or("").as_bytes();
        unsafe { wasm_tombstone_early(bytes.as_ptr(), bytes.len(), message.is_some()); }
        intrinsics::abort();
    }
    let must_abort = panic_count::increase(true);`;

/** 锁定 std 源码共同入口；版本漂移或重复打补丁时显式失败。 */
export function patchPanicSource(source) {
  if (source.includes(marker) || source.split(anchor).length !== 2) throw new Error("std panic 入口缺失、重复或已打补丁");
  return source.replace(anchor, replacement) + `
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn wasm_tombstone_std_marker() -> u32 { 0x57325431 }
`;
}
