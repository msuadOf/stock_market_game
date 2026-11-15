use super::*;

fn header() -> TombstoneHeader {
    TombstoneHeader {
        worker_id: 17,
        phase: 3,
        kind: DeathKind::Panic,
    }
}

#[test]
fn 固定布局与初始无记录() {
    use std::mem::{align_of, offset_of, size_of};
    assert_eq!(size_of::<TombstoneRecord>(), 256);
    assert_eq!(align_of::<TombstoneRecord>(), 4);
    assert_eq!(
        [
            offset_of!(TombstoneRecord, magic),
            offset_of!(TombstoneRecord, worker_id),
            offset_of!(TombstoneRecord, phase),
            offset_of!(TombstoneRecord, kind),
            offset_of!(TombstoneRecord, truncated),
            offset_of!(TombstoneRecord, len),
            offset_of!(TombstoneRecord, reserved),
            offset_of!(TombstoneRecord, payload),
        ],
        [0, 4, 8, 10, 11, 12, 14, 16]
    );
    assert_eq!(TombstoneRecord::new().read(), TombstoneRead::Idle);
}

#[test]
fn 唯一写权与半发布有限读取() {
    let record = TombstoneRecord::new();
    let writer = record.try_claim(header()).expect("首个写者");
    assert!(record.try_claim(header()).is_none());
    assert_eq!(record.read(), TombstoneRead::Writing);
    writer.publish(b"original");
    assert!(record.try_claim(header()).is_none());
    let TombstoneRead::Live(snapshot) = record.read() else {
        panic!("必须发布")
    };
    assert_eq!(snapshot.header, header());
    assert_eq!(&snapshot.payload[..snapshot.len as usize], b"original");
    assert!(!snapshot.truncated);
}

#[test]
fn 遗弃写者不等待也不编造硬trap() {
    let record = TombstoneRecord::new();
    drop(record.try_claim(header()).expect("首个写者"));
    assert_eq!(record.read(), TombstoneRead::Writing);
    assert!(record.try_claim(header()).is_none());
}

#[test]
fn 空与边界原始字节保真且只截断超长() {
    for length in [0, 239, 240, 241, 1024] {
        let record = TombstoneRecord::new();
        let payload = [0xff; 1024];
        record
            .try_claim(header())
            .expect("首个写者")
            .publish(&payload[..length]);
        let TombstoneRead::Live(snapshot) = record.read() else {
            panic!("必须发布")
        };
        assert_eq!(snapshot.len as usize, length.min(240));
        assert_eq!(snapshot.truncated, length > 240);
        assert_eq!(
            &snapshot.payload[..snapshot.len as usize],
            &payload[..length.min(240)]
        );
        assert!(snapshot.payload[snapshot.len as usize..]
            .iter()
            .all(|&byte| byte == 0));
    }
}

#[test]
fn 并发竞争只发布获胜者完整快照() {
    let record = TombstoneRecord::new();
    std::thread::scope(|scope| {
        for id in 1..=16 {
            let record = &record;
            scope.spawn(move || {
                if let Some(writer) = record.try_claim(TombstoneHeader {
                    worker_id: id,
                    phase: id as u16,
                    kind: DeathKind::Panic,
                }) {
                    writer.publish(&[id as u8; 241]);
                }
            });
        }
    });
    let TombstoneRead::Live(snapshot) = record.read() else {
        panic!("必须发布")
    };
    assert!((1..=16).contains(&snapshot.header.worker_id));
    assert_eq!(snapshot.header.phase as u32, snapshot.header.worker_id);
    assert_eq!(snapshot.payload, [snapshot.header.worker_id as u8; 240]);
    assert!(snapshot.truncated);
}

#[test]
fn 损坏字段显式返回而不panic或静默缺省() {
    let record = TombstoneRecord::new();
    record.magic.store(9, Ordering::Relaxed);
    assert_eq!(
        record.read(),
        TombstoneRead::Invalid(TombstoneInvalid::Magic(9))
    );
    record.magic.store(IDLE, Ordering::Relaxed);
    record
        .try_claim(header())
        .expect("首个写者")
        .publish(b"payload");
    record.kind.store(9, Ordering::Relaxed);
    assert_eq!(
        record.read(),
        TombstoneRead::Invalid(TombstoneInvalid::Kind(9))
    );
    record.kind.store(1, Ordering::Relaxed);
    record.len.store(241, Ordering::Relaxed);
    assert_eq!(
        record.read(),
        TombstoneRead::Invalid(TombstoneInvalid::Length(241))
    );
    record.len.store(7, Ordering::Relaxed);
    record.truncated.store(2, Ordering::Relaxed);
    assert_eq!(
        record.read(),
        TombstoneRead::Invalid(TombstoneInvalid::Truncated(2))
    );
    record.truncated.store(1, Ordering::Relaxed);
    assert_eq!(
        record.read(),
        TombstoneRead::Invalid(TombstoneInvalid::TruncatedLength(7))
    );
    record.truncated.store(0, Ordering::Relaxed);
    record.reserved.store(1, Ordering::Relaxed);
    assert_eq!(
        record.read(),
        TombstoneRead::Invalid(TombstoneInvalid::Reserved(1))
    );
}

#[test]
fn 显式硬trap种类仅由写者提供() {
    let record = TombstoneRecord::new();
    let header = TombstoneHeader {
        kind: DeathKind::HardTrap,
        ..header()
    };
    record.try_claim(header).expect("首个写者").publish(b"");
    let TombstoneRead::Live(snapshot) = record.read() else {
        panic!("必须发布")
    };
    assert_eq!(snapshot.header, header);
    assert_eq!(snapshot.len, 0);
}

#[test]
fn 死亡信号先于记录发布且重复写不能覆盖() {
    let record = TombstoneRecord::new();
    let header = TombstoneHeader {
        worker_id: 12,
        phase: 0,
        kind: DeathKind::Panic,
    };
    let mut signal_seen = false;
    record.signal_then_publish(header, "故障".as_bytes(), || {
        assert_eq!(record.read(), TombstoneRead::Idle);
        signal_seen = true;
    });
    assert!(signal_seen);
    let first = record.read();
    let mut second_signal_seen = false;
    record.signal_then_publish(header, "重复".as_bytes(), || {
        second_signal_seen = true;
    });
    assert!(second_signal_seen);
    assert_eq!(record.read(), first);
}

#[test]
fn 真实join死亡字先发布再通知然后写入诊断() {
    use std::sync::atomic::{AtomicU32, Ordering};
    let record = TombstoneRecord::new();
    let join = AtomicU32::new(0x100);
    let dead_code = AtomicU32::new(0);
    let target = PanicTarget {
        record: &record,
        join: &join,
        dead_code: &dead_code,
        waiting: 0x100,
        writing: 0x101,
        dead: 0x104,
        header: TombstoneHeader {
            worker_id: 3,
            phase: 0,
            kind: DeathKind::Panic,
        },
    };
    let mut notified = false;
    target.publish(b"panic", || {
        assert_eq!(join.load(Ordering::SeqCst), 0x104);
        assert_eq!(dead_code.load(Ordering::SeqCst), 1);
        assert_eq!(record.read(), TombstoneRead::Idle);
        notified = true;
    });
    assert!(notified);
    assert!(matches!(record.read(), TombstoneRead::Live(_)));
}

#[test]
fn 已结束或换代的join不能被死亡信号覆盖() {
    use std::sync::atomic::{AtomicU32, Ordering};
    let record = TombstoneRecord::new();
    let join = AtomicU32::new(0x202);
    let dead_code = AtomicU32::new(9);
    let target = PanicTarget {
        record: &record,
        join: &join,
        dead_code: &dead_code,
        waiting: 0x100,
        writing: 0x101,
        dead: 0x104,
        header: TombstoneHeader {
            worker_id: 3,
            phase: 0,
            kind: DeathKind::Panic,
        },
    };
    target.publish(b"panic", || {});
    assert_eq!(join.load(Ordering::SeqCst), 0x202);
    assert_eq!(dead_code.load(Ordering::SeqCst), 9);
}
