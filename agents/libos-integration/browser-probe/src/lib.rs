use engine::verification;
use std::sync::atomic::{AtomicU32, Ordering};

const VERIFICATION_BUSY: u32 = 5;

fn verification_is_busy(error: &str) -> bool {
    error.starts_with("LIBOS_VERIFICATION_BUSY:")
}

/// 为 browser harness 分配的检查数组；session 本身仍是 LibOS registry 校验的 opaque ID。
#[no_mangle]
pub extern "C" fn probe_prepare(mode: u32) -> u32 {
    let layout = verification::prepare(mode)
        .unwrap_or_else(|error| panic!("LibOS prepare 失败：{error}"));
    Box::into_raw(Box::new([layout.session, layout.wait_word, layout.active])) as u32
}

/// 为多槽故障场景导出同一 FaultDomain 下的多个真实 LibOS 通道布局。
#[no_mangle]
pub extern "C" fn probe_prepare_group(count: u32) -> u32 {
    let layouts = verification::prepare_group(count)
        .unwrap_or_else(|error| panic!("LibOS group prepare 失败：{error}"));
    let mut pointers = Vec::with_capacity(1 + layouts.len());
    pointers.push(u32::try_from(layouts.len()).expect("verification group 成员数超出 u32"));
    for layout in layouts {
        let layout = Box::new([layout.session, layout.wait_word, layout.active]);
        pointers.push(Box::into_raw(layout) as u32);
    }
    Box::into_raw(pointers.into_boxed_slice()) as *mut u32 as u32
}

#[no_mangle]
pub unsafe extern "C" fn probe_send(session: u32, value: u32) {
    verification::send(session, value)
        .unwrap_or_else(|error| panic!("LibOS send 失败：{error}"));
}

#[no_mangle]
pub unsafe extern "C" fn probe_disconnect(session: u32) {
    verification::disconnect(session)
        .unwrap_or_else(|error| panic!("LibOS disconnect 失败：{error}"));
}

#[no_mangle]
pub unsafe extern "C" fn probe_abort(session: u32) {
    verification::abort(session)
        .unwrap_or_else(|error| panic!("LibOS abort 失败：{error}"));
}

#[no_mangle]
pub unsafe extern "C" fn probe_stop(session: u32) {
    verification::stop(session)
        .unwrap_or_else(|error| panic!("LibOS stop 失败：{error}"));
}

#[no_mangle]
pub unsafe extern "C" fn probe_receive(session: u32, timeout_ms: u32, output: u32) -> u32 {
    let result = output as *mut [u32; 3];
    let (status, value, waited_ms) = match verification::run_receive(session, timeout_ms) {
        Ok(result) => result,
        Err(error) if verification_is_busy(&error) => {
            result.write([VERIFICATION_BUSY, 0, 0]);
            return VERIFICATION_BUSY;
        }
        Err(error) => panic!("LibOS receive 失败：{error}"),
    };
    result.write([status, value, waited_ms]);
    status
}

#[no_mangle]
pub unsafe extern "C" fn probe_phase_receive(
    session: u32,
    phase: u32,
    normal_ms: u32,
    long_ms: u32,
    output: u32,
) -> u32 {
    let result = output as *mut [u32; 4];
    let (status, value, waited_ms, returned_phase) =
        match verification::run_phase_receive(session, phase, normal_ms, long_ms) {
            Ok(result) => result,
            Err(error) if verification_is_busy(&error) => {
                result.write([VERIFICATION_BUSY, 0, 0, phase]);
                return VERIFICATION_BUSY;
            }
            Err(error) => panic!("LibOS phase receive 失败：{error}"),
        };
    result.write([status, value, waited_ms, returned_phase]);
    status
}

#[no_mangle]
pub unsafe extern "C" fn probe_try_receive(session: u32, output: u32) -> u32 {
    let result = output as *mut [u32; 2];
    let (status, value) = match verification::try_receive(session) {
        Ok(result) => result,
        Err(error) if verification_is_busy(&error) => {
            result.write([VERIFICATION_BUSY, 0]);
            return VERIFICATION_BUSY;
        }
        Err(error) => panic!("LibOS try_receive 失败：{error}"),
    };
    result.write([status, value]);
    status
}

#[no_mangle]
pub extern "C" fn probe_alloc_words(length: u32) -> u32 {
    assert!(length > 0, "探针结果缓冲区长度必须大于零");
    let words = vec![0u32; length as usize].into_boxed_slice();
    Box::into_raw(words) as *mut u32 as u32
}

#[no_mangle]
pub unsafe extern "C" fn probe_free_words(pointer: u32, length: u32) {
    assert!(length > 0, "探针结果缓冲区长度必须大于零");
    let slice = std::ptr::slice_from_raw_parts_mut(pointer as *mut u32, length as usize);
    drop(Box::from_raw(slice));
}

#[no_mangle]
pub unsafe extern "C" fn probe_finish(layout_ptr: u32) {
    let layout = &*(layout_ptr as *const [u32; 3]);
    verification::finish(layout[0])
        .unwrap_or_else(|error| panic!("LibOS finish 失败：{error}"));
    drop(Box::from_raw(layout_ptr as *mut [u32; 3]));
}

static SENTINEL: AtomicU32 = AtomicU32::new(0);

#[no_mangle]
pub extern "C" fn probe_sentinel_set(value: u32) {
    SENTINEL.store(value, Ordering::SeqCst);
}

#[no_mangle]
pub extern "C" fn probe_sentinel_get() -> u32 {
    SENTINEL.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn probe_stack() -> u32 {
    let stack = vec![0u128; 32768].into_boxed_slice();
    let top = (stack.as_ptr() as usize) + std::mem::size_of_val(&*stack);
    std::mem::forget(stack);
    u32::try_from(top).expect("测试栈地址超出 wasm32")
}

#[no_mangle]
pub extern "C" fn probe_hard_trap() {
    core::arch::wasm32::unreachable();
}

