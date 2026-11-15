//! 单复合字 W 编解码（§12.1.1/§13.1）。
//!
//! **布局**：`W = gen << (32−gen_bits) | state`（u32 单字）。身份与状态
//! 同字——一切 CAS 与 Atomics.wait 的期望值 = 完整 W 字（I1 单字原子、
//! I2 期望钉代），"读到 gen 与读到 state 属不同代"的交错在字面上不存在。
//! 默认 `gen_bits=29`（state 占低 3 位、恰 8 值）；测试缩空间取小值
//! （§13.3 T-耗），单 u32 复合字性质不变（I1/I2/I3 论证与位宽无关）。
//!
//! **状态编码（§13.1 重排）**：数值保序 ⇒ I3 单调无 ABA（同 gen 内一切
//! 转移严格递增；换代由 gen 位 +1 支配恒增；gen 耗尽转 RETIRED 不绕回）。
//!
//! TS 镜像：`apps/web/src/host/facade-slot-protocol.ts`（常量互为 tripwire，
//! 由其测试解析本文件核对）。

/// 请求槽状态（3 bit、≤8 值；新增状态超 8 需过设计门改位宽）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SlotState {
    /// 新槽首用前（W 初值 0 = (gen 0, Idle)）。
    Idle = 0,
    /// H4 已发布（唯一 Release 发布点）。
    Req = 1,
    /// A1 已领写权（应答在途；取消窗关闭，§13.1 A4）。
    Answering = 2,
    /// A3 终态：子构造成功（载荷 worker_id）。
    Ok = 3,
    /// A3 终态：构造失败（载荷 errcode；等 engine ACK 后自回收）。
    Fail = 4,
    /// engine 终态：ACK 超时取消（无载荷写；即刻自回收）。
    Cancelled = 5,
    /// engine 终态：FAIL 载荷已消化（即刻自回收）。
    FailAcked = 6,
    /// RB 退休标记（gen 耗尽槽专用，§13.3；不入状态机转移表、永不复用）。
    Retired = 7,
}

/// join 槽状态（§9.2.3 单向状态机；同一 W 布局、独立枚举）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum JoinState {
    /// H3 发布时复位。
    Waiting = 0,
    /// D5 已领载荷写权；code 写完后发布 DEAD，join 等待。
    DeadWriting = 1,
    /// W2：trampoline 正常出口（result 已写、Release）。
    Done = 2,
    /// D5：supervisor 死亡信号（硬 trap）。
    DeadHardtrap = 3,
    /// panic hook 死亡信号。
    DeadPanic = 4,
    /// JoinHandle::join 消费后（迟到写者 CAS 必败）。
    Consumed = 5,
}

/// 默认 gen 位宽：29（state 恰 3 bit）。
pub const GEN_BITS_DEFAULT: u32 = 29;
/// state 位宽上限（gen_bits 下界）：3 bit、8 值。
pub const STATE_BITS_MAX: u32 = 3;

/// 状态全集（8 值，含 Retired 退休标记）。
pub const fn all_slot_states() -> [SlotState; 8] {
    [
        SlotState::Idle,
        SlotState::Req,
        SlotState::Answering,
        SlotState::Ok,
        SlotState::Fail,
        SlotState::Cancelled,
        SlotState::FailAcked,
        SlotState::Retired,
    ]
}

/// join 状态全集（6 值，含载荷写入中间态）。
pub const fn all_join_states() -> [JoinState; 6] {
    [
        JoinState::Waiting,
        JoinState::Done,
        JoinState::DeadHardtrap,
        JoinState::DeadPanic,
        JoinState::Consumed,
        JoinState::DeadWriting,
    ]
}

/// gen_bits 合法域：[1, 29]（gen 域 ≥1 位且 state 域 ≥3 位——state 承载
/// 8 值）。上界 `32 − STATE_BITS_MAX = 29`；下界 1 保证 gen 域存在。
///
/// 说明（复核订正文档）：旧注释把域写成 `[STATE_BITS_MAX, 29]` 且实现写
/// `>= 1` 看似矛盾，实际二者都对——下界是 gen ≥1（gen_bits≥1），state 域
/// 由 `32 − gen_bits ≥ 3` 约束 ⇒ gen_bits ≤ 29。故合法域是 [1, 29]。
pub const fn gen_bits_valid(gen_bits: u32) -> bool {
    gen_bits >= 1 && gen_bits <= 32 - STATE_BITS_MAX
}

/// gen 合法上界（含）：2^gen_bits − 1；达到后复用即退休（§13.3 RB）。
pub const fn gen_max(gen_bits: u32) -> u32 {
    // gen_bits ≤ 29 ⇒ 无溢出。
    (1u64 << gen_bits) as u32 - 1
}

/// 编码请求槽复合字 W = gen<<(32−gen_bits)|state。
///
/// 编译期/运行期双重钉住（复核修正）：此前注释声称"debug_assert 钉住"但
/// 实为无断言，越界 gen 会静默重叠到 state 域——而这正是 I2（期望钉代）
/// 的立足点，静默截断等于绕过跨代身份检查。改为 const fn 内 assert!
/// （常量求值期即失败；运行期调用亦有断言）。
pub const fn encode_w(gen: u32, state: SlotState, gen_bits: u32) -> u32 {
    assert!(
        gen_bits_valid(gen_bits),
        "encode_w: gen_bits 越界（须 1..=29）"
    );
    assert!(gen <= gen_max(gen_bits), "encode_w: gen 超出 gen_bits 域");
    (gen << (32 - gen_bits)) | (state as u32)
}

/// 编码 join 槽复合字（同一布局；断言同 `encode_w`）。
pub const fn encode_join(gen: u32, state: JoinState, gen_bits: u32) -> u32 {
    assert!(
        gen_bits_valid(gen_bits),
        "encode_join: gen_bits 越界（须 1..=29）"
    );
    assert!(
        gen <= gen_max(gen_bits),
        "encode_join: gen 超出 gen_bits 域"
    );
    (gen << (32 - gen_bits)) | (state as u32)
}

/// 解码 gen（高位）。
pub const fn decode_gen(w: u32, gen_bits: u32) -> u32 {
    w >> (32 - gen_bits)
}

/// 解码请求槽状态；低位域含 8 值之外杂散位 = 协议破坏 ⇒ 显式 panic
/// （铁律二：不静默截断——掩盖等价于绕过跨代身份检查）。
pub fn decode_state(w: u32, gen_bits: u32) -> SlotState {
    let raw = w & gen_state_mask(gen_bits);
    match raw {
        0 => SlotState::Idle,
        1 => SlotState::Req,
        2 => SlotState::Answering,
        3 => SlotState::Ok,
        4 => SlotState::Fail,
        5 => SlotState::Cancelled,
        6 => SlotState::FailAcked,
        7 => SlotState::Retired,
        // gen_bits=29 时 raw ≤ 7 不可达；缩空间下杂散位达此。
        _ => panic!(
            "非法 SlotState 位形：低 {} 位值 {raw}（协议破坏，须排查写者）",
            32 - gen_bits
        ),
    }
}

/// 解码 join 槽状态（非法位形显式 panic，同上）。
pub fn decode_join_state(w: u32, gen_bits: u32) -> JoinState {
    let raw = w & gen_state_mask(gen_bits);
    match raw {
        0 => JoinState::Waiting,
        1 => JoinState::DeadWriting,
        2 => JoinState::Done,
        3 => JoinState::DeadHardtrap,
        4 => JoinState::DeadPanic,
        5 => JoinState::Consumed,
        _ => panic!("非法 JoinState 位形：低 {} 位值 {raw}", 32 - gen_bits),
    }
}

/// state 域掩码（低位 32−gen_bits 位）。
pub const fn gen_state_mask(gen_bits: u32) -> u32 {
    // 32−gen_bits ≥ 3 ⇒ 移位安全。
    if 32 - gen_bits >= 32 {
        u32::MAX
    } else {
        (1u32 << (32 - gen_bits)) - 1
    }
}
