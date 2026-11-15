//! Browser join 合同探针。只在 verification-harness 构建，用实际公共 WorkerHandle。

use libos::thread_facade::WorkerHandle;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

thread_local! {
    static HANDLE: RefCell<Option<WorkerHandle<u32>>> = const { RefCell::new(None) };
}

#[wasm_bindgen]
pub fn verification_worker_handle_spawn(input: u32) -> Result<u32, JsValue> {
    HANDLE.with(|slot| {
        let mut owner = slot.borrow_mut();
        if owner.is_some() {
            return Err(JsValue::from_str("WorkerHandle 合同探针已有在途句柄"));
        }
        let handle = WorkerHandle::spawn("verification-owned-worker-handle".to_owned(), move || {
            input ^ 0x5A5A
        })
        .map_err(|error| JsValue::from_str(&format!("WorkerHandle::spawn 失败：{error}")))?;
        let join_index = handle.join_index();
        *owner = Some(handle);
        Ok(join_index)
    })
}

#[wasm_bindgen]
pub fn verification_worker_handle_join() -> Result<u32, JsValue> {
    let handle = HANDLE
        .with(|slot| slot.borrow_mut().take())
        .ok_or_else(|| JsValue::from_str("WorkerHandle 合同探针无在途句柄"))?;
    handle
        .join()
        .map_err(|error| JsValue::from_str(&format!("WorkerHandle::join 失败：{error}")))
}
