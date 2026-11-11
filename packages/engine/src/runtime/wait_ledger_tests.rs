//! 等待点分层账·静态锚测试（wasm 线程抽象层 R4a 阶段 1，C6）。
//!
//! 设计锚：`.tmp/wasm-thread-abstraction-design-input.md` §10.1（两层等待
//! 模型 + 锁序图）与 §11.1（deferred_drop 通道 (o)(p)(q) 三元组）。
//!
//! 把"业务层非阻塞 vs 原语内部等待"的两层分离**静态化**：本文件不重述
//! 设计论证，只钉住设计依赖的**源码形态不变量**——任何一条被后续改动
//! 破坏（例如在持 capture_slot 锁的临界区内 send、deferred_drop 提交面
//! 退化为阻塞 send、账锚注释被误删），本测试显式红，防止锁序图与等待
//! 点账在设计不知情下失效。锚注释本体在源文件内（`等待点账` 标记），
//! 本测试只验证锚在位，不复制注释内容（避免双源漂移）。

/// world.rs / desk.rs / group.rs 的 capture_slot 临界区（lock→clone→释放，
/// §10.1.3 锁序图：capture_slot 只进 (l) alloc 锁叶，不进 (m)/(j)/槽）不得
/// 出现 `send(` ——「持 capture_slot 时 send」会造出设计未论证的锁序边。
#[test]
fn 等待点账_capture_slot临界区不得send() {
    // with_capture（D4 捕获绑定安装点）只存在于 desk.rs / group.rs 的
    // worker 主循环；world.rs 是协调者侧（capture_slot 直接 lock 赋值两处）。
    for (file, source, has_with_capture) in [
        ("world.rs", include_str!("world.rs"), false),
        ("desk.rs", include_str!("desk.rs"), true),
        ("group.rs", include_str!("group.rs"), true),
    ] {
        for (index, line) in source.lines().enumerate() {
            let contains_capture_lock = line.contains("capture_slot.lock()")
                || (line.contains(".lock()") && line.contains("capture"));
            assert!(
                !(contains_capture_lock && line.contains(".send(")),
                "等待点账违例：{file} 第 {} 行在 capture 锁语句内出现 send：{}",
                index + 1,
                line.trim()
            );
        }
        // with_capture：锁临界区=lock→clone→释放（D4），work 闭包在锁外执行
        // （with_phase_timing_binding 安装绑定后运行）——函数体内不得有 send
        // 调用字面（新增即须重过锁序论证并更新设计 §10.1.3）。
        if has_with_capture {
            let start = source.find("fn with_capture").unwrap_or_else(|| {
                panic!("等待点账锚缺失：{file} 缺少 with_capture（D4 捕获绑定安装点）")
            });
            let rest = &source[start..];
            let end = rest.find("\n    }\n").unwrap_or(rest.len());
            let body = &rest[..end];
            assert!(
                !body.contains(".send("),
                "等待点账违例：{file} 的 with_capture 函数体内出现 send——锁序图（§10.1.3）未论证此边"
            );
            assert!(
                body.contains(".clone()"),
                "等待点账锚缺失：{file} 的 with_capture 临界区应只 clone"
            );
        }
    }
}

/// deferred_drop 有界通道（sync_channel(64) = mpmc array flavor）提交面
/// 只允许 `try_send`（满=FullInline 就地析构背压，§11.1：「阻塞 send
/// 生产不可达」）；出现非 try 的 `sender.send(` 即账外新等待点 (o) 之外的
/// 阻塞形态，必须先入设计账再实现。
#[test]
fn 等待点账_deferred_drop提交面只允许try_send() {
    let source = include_str!("../session/deferred_drop.rs");
    for (index, line) in source.lines().enumerate() {
        let sender_call = line.contains("sender.try_send") || line.contains("sender.send");
        if sender_call {
            assert!(
                line.contains("try_send"),
                "等待点账违例：deferred_drop 第 {} 行出现阻塞 send：{}（设计 §11.1：阻塞 send 生产不可达，只允许 try_send）",
                index + 1,
                line.trim()
            );
        }
    }
    assert!(
        source.contains("try_send"),
        "等待点账锚缺失：deferred_drop 缺少 try_send 提交面"
    );
}

/// 账锚在位性：各等待点所在模块必须携带 `等待点账` 标记注释（业务层/
/// 原语层两层分层与 (l)-(q) 三元组的源内登记位）。锚被删=设计账与代码
/// 失联，显式红。
#[test]
fn 等待点账_锚注释在位() {
    let anchors: &[(&str, &str)] = &[
        // (模块文件, 锚关键片段)——片段取自账注释的稳定关键词。
        ("world.rs", "等待点账"),
        ("wait_contract.rs", "等待点账"),
        ("../session/deferred_drop.rs", "等待点账"),
        ("../compute.rs", "等待点账"),
    ];
    for (file, marker) in anchors {
        let source = match *file {
            "world.rs" => include_str!("world.rs"),
            "wait_contract.rs" => include_str!("wait_contract.rs"),
            "../session/deferred_drop.rs" => include_str!("../session/deferred_drop.rs"),
            "../compute.rs" => include_str!("../compute.rs"),
            _ => unreachable!(),
        };
        assert!(
            source.contains(marker),
            "等待点账锚缺失：{file} 缺少「{marker}」标记注释——等待点分层账与源码失联"
        );
    }
}
