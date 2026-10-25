//! W 编解码测试（设计锚 §12.1.1 / §13.1 状态重排 / §13.3 gen_bits 缩空间）。

use crate::facade_slot::codec::*;

/// §13.1 状态编码重排后的字面值——与 TS 侧 `facade-slot-protocol.ts` 镜像
/// 常量互为 tripwire（跨语言漂移由 TS 测试解析本文件常量核对）。
#[test]
fn 状态枚举值与设计表一致() {
    assert_eq!(SlotState::Idle as u32, 0);
    assert_eq!(SlotState::Req as u32, 1);
    assert_eq!(SlotState::Answering as u32, 2);
    assert_eq!(SlotState::Ok as u32, 3);
    assert_eq!(SlotState::Fail as u32, 4);
    assert_eq!(SlotState::Cancelled as u32, 5);
    assert_eq!(SlotState::FailAcked as u32, 6);
    // RB 退休标记（§13.3：耗尽槽转 RETIRED 永不复用；第 8 个值，超 8 需过设计门）。
    assert_eq!(SlotState::Retired as u32, 7);
}

/// join 槽状态（§9.2.3 单向状态机；同一 W 布局、独立枚举）。
#[test]
fn join状态枚举值单调() {
    assert_eq!(JoinState::Waiting as u32, 0);
    assert_eq!(JoinState::Done as u32, 2);
    assert_eq!(JoinState::DeadHardtrap as u32, 3);
    assert_eq!(JoinState::DeadPanic as u32, 4);
    assert_eq!(JoinState::Consumed as u32, 5);
}

/// §12.1.1：W = gen<<3|state（默认 gen_bits=29）；新槽首用 gen=0、W 初值 0。
#[test]
fn 复合字布局与往返() {
    assert_eq!(GEN_BITS_DEFAULT, 29);
    // 新槽初值 = (gen 0, Idle) = 0。
    assert_eq!(encode_w(0, SlotState::Idle, GEN_BITS_DEFAULT), 0);
    // 身份与状态同字：读 W 即同时读到 (gen,state)。
    for gen in [0u32, 1, 7, 0x1234_5678 >> 3, (1 << 29) - 1] {
        for state in all_slot_states() {
            let w = encode_w(gen, state, GEN_BITS_DEFAULT);
            assert_eq!(decode_gen(w, GEN_BITS_DEFAULT), gen);
            assert_eq!(decode_state(w, GEN_BITS_DEFAULT), state);
        }
    }
    // 具体位形锚：gen=7,state=Req ⇒ 7<<3|1。
    assert_eq!(encode_w(7, SlotState::Req, GEN_BITS_DEFAULT), 7 << 3 | 1);
}

/// §13.3：gen_bits 测试缩空间——W 布局改 gen<<(32−gen_bits)|state，单 u32
/// 复合字性质不变（I1/I2/I3 与位宽无关）。
#[test]
fn gen_bits缩空间布局() {
    let bits = 4u32;
    // gen 上界 2^4−1=15；state 落低 28 位。
    assert_eq!(encode_w(15, SlotState::Fail, bits), 15 << 28 | 4);
    let w = encode_w(9, SlotState::Answering, bits);
    assert_eq!(decode_gen(w, bits), 9);
    assert_eq!(decode_state(w, bits), SlotState::Answering);
    // gen_bits=4 时 state 高位为 0、不与 gen 位重叠。
    for state in all_slot_states() {
        let w = encode_w(15, state, bits);
        assert_eq!(w >> 28, 15);
        assert_eq!(w & 0x0FFF_FFFF, state as u32);
    }
}

/// §13.1 I3 单调无 ABA：一切转移数值严格递增（逐边核对设计表）。
#[test]
fn 转移表全边严格递增() {
    let g = 29;
    let up = |from: SlotState, to: SlotState| {
        assert!(
            encode_w(0, to, g) > encode_w(0, from, g),
            "转移 {from:?}→{to:?} 未严格递增"
        );
    };
    // 请求槽状态机（分配换代边由 gen 位 +1 支配恒增，另行断言）。
    up(SlotState::Req, SlotState::Answering);
    up(SlotState::Answering, SlotState::Ok);
    up(SlotState::Answering, SlotState::Fail);
    up(SlotState::Req, SlotState::Cancelled);
    up(SlotState::Fail, SlotState::FailAcked);
    // 分配 H4：(g,任一非终态/终态)→(g+1,REQ) 与新槽 0→(0,REQ)。
    let any_state = SlotState::FailAcked; // 终态最大值代表
    assert!(encode_w(1, SlotState::Req, g) > encode_w(0, any_state, g));
    assert!(encode_w(1, SlotState::Req, g) > encode_w(0, SlotState::Idle, g));
    // join 槽状态机：WAITING→{DONE|DEAD_*}→CONSUMED。
    for dead in [
        JoinState::Done,
        JoinState::DeadHardtrap,
        JoinState::DeadPanic,
    ] {
        assert!(
            encode_join(0, dead, g) > encode_join(0, JoinState::Waiting, g),
            "join 转移 Waiting→{dead:?} 未严格递增"
        );
        assert!(
            encode_join(0, JoinState::Consumed, g) > encode_join(0, dead, g),
            "join 转移 {dead:?}→Consumed 未严格递增"
        );
    }
}

/// decode_state 对非法状态位形显式 panic（铁律二：不静默映射、不截断）。
/// gen_bits=29 时低 3 位 0..7 恰为全集、不可能非法；缩空间（gen_bits<29）
/// 时 state 域更宽，位 3 起的杂散位必须显式拒绝（掩盖 = 跨代身份检查被绕过）。
#[test]
#[should_panic(expected = "非法 SlotState 位形")]
fn 非法状态位形显式失败() {
    let bits = 4u32;
    // gen=0、低 28 位杂散值 8（超出 8 值状态全集）。
    let w = 0u32 | 8;
    let _ = decode_state(w, bits);
}

/// 常量口径：all_slot_states 覆盖 8 值（含 Retired 退休标记）。
#[test]
fn 状态全集恰八值() {
    assert_eq!(all_slot_states().len(), 8);
    assert_eq!(all_join_states().len(), 6);
}

#[test]
fn join载荷写权中间态仍单调且非法位形拒绝() {
    let gb = GEN_BITS_DEFAULT;
    for dead in [JoinState::DeadHardtrap, JoinState::DeadPanic] {
        assert!(
            encode_join(0, JoinState::Waiting, gb) < encode_join(0, JoinState::DeadWriting, gb)
        );
        assert!(encode_join(0, JoinState::DeadWriting, gb) < encode_join(0, dead, gb));
        assert!(encode_join(0, dead, gb) < encode_join(0, JoinState::Consumed, gb));
    }
    for raw in [6, 7] {
        assert!(std::panic::catch_unwind(|| decode_join_state(raw, gb)).is_err());
    }
}
