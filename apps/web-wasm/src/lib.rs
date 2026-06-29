//! engine → WASM 绑定（ADR-0007 §4）。
//!
//! 多核：wasm-bindgen-rayon 初始化线程池后，engine 内的 rayon par_iter
//! 自动用满浏览器所有核心（SharedArrayBuffer + Atomics）。
//!
//! GameSession 不可序列化（含 `Box<dyn Strategy + Send + Sync>`），故存于 thread_local
//! 句柄注册表；仅 `Snapshot`/`Event`/`Intent`/`SessionSetup` 经 serde-wasm-bindgen 跨界。
//! JS 持 u32 句柄调 create/step/snapshot/enqueue/drop。
//!
//! 纯前端单机：player 固定 AccountId(0)（enqueue 不带 player_id）。

use engine::{AccountId, GameSession, Intent, SaveSlot, SessionSetup};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use wasm_bindgen::prelude::*;

/// 初始化 WASM 多线程（wasm-bindgen-rayon）。
/// 必须在 create_session 前调用。浏览器需启用 SharedArrayBuffer（COOP/COEP 头）。
#[wasm_bindgen]
pub fn init_threads(cores: u32) {
    wasm_bindgen_rayon::init_thread_pool(cores as usize);
}

/// 序列化为 JsValue。map 默认序列化为 JS Map（AccountId 是数字键，无法作 Object 键）；
/// 前端 host 适配器负责把 Map 规整为普通对象（Object.fromEntries）供 React/RTK 消费。
fn to_js<T: Serialize>(v: &T) -> Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(v).map_err(|e| JsValue::from_str(&e.to_string()))
}

thread_local! {
    static REGISTRY: RefCell<HashMap<u32, GameSession>> = RefCell::new(HashMap::new());
}
static NEXT: AtomicU32 = AtomicU32::new(1);

/// 创建会话。setup 为 SessionSetup 的 JS 对象，seed 为种子。
/// 返回句柄 u32。失败（setup 非法）→ 抛 JsValue。
#[wasm_bindgen]
pub fn create_session(setup: JsValue, seed: u64) -> Result<u32, JsValue> {
    let setup: SessionSetup = serde_wasm_bindgen::from_value(setup)?;
    let sess = GameSession::new(setup, seed).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    REGISTRY.with(|r| r.borrow_mut().insert(id, sess));
    Ok(id)
}

/// 推进一个 tick，返回 Event[]（带 seq）。
#[wasm_bindgen]
pub fn step(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |sess| {
        let events = sess.step();
        Ok(to_js(&events)?)
    })
}

/// 拉完整快照（首次连/重连/存档）。
#[wasm_bindgen]
pub fn snapshot(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |sess| Ok(to_js(&sess.snapshot())?))
}

/// 当前 tick（已推进数）。
#[wasm_bindgen]
pub fn tick(handle: u32) -> Result<u64, JsValue> {
    with_session(handle, |sess| Ok(sess.tick()))
}

/// 当前交易日。
#[wasm_bindgen]
pub fn day(handle: u32) -> Result<u32, JsValue> {
    with_session(handle, |sess| Ok(sess.day()))
}

/// 玩家入队意图（player 固定 AccountId(0)，单机纯前端）。intent 为 Intent 的 JS 对象。
#[wasm_bindgen]
pub fn enqueue(handle: u32, intent: JsValue) -> Result<(), JsValue> {
    let intent: Intent = serde_wasm_bindgen::from_value(intent)?;
    with_session(handle, |sess| {
        sess.enqueue_player_intent(AccountId(0), intent)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// 销毁会话（释放内存）。
#[wasm_bindgen]
pub fn drop_session(handle: u32) {
    REGISTRY.with(|r| {
        r.borrow_mut().remove(&handle);
    });
}

/// 生成存档（精确到交易日）。返回 SaveSlot 的 JS 对象。
#[wasm_bindgen]
pub fn save(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |sess| {
        let slot = sess.save();
        Ok(to_js(&slot)?)
    })
}

/// 从存档恢复（精确到天）。返回新句柄。
#[wasm_bindgen]
pub fn restore(save_slot: JsValue) -> Result<u32, JsValue> {
    let slot: SaveSlot = serde_wasm_bindgen::from_value(save_slot)?;
    let sess = GameSession::restore(&slot).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    REGISTRY.with(|r| r.borrow_mut().insert(id, sess));
    Ok(id)
}

/// 句柄内执行闭包；句柄无效 → 抛 JsValue。
fn with_session<T>(
    handle: u32,
    f: impl FnOnce(&mut GameSession) -> Result<T, JsValue>,
) -> Result<T, JsValue> {
    REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        match reg.get_mut(&handle) {
            Some(sess) => f(sess),
            None => Err(JsValue::from_str(&format!("invalid session handle: {handle}"))),
        }
    })
}
