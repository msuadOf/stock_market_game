use super::*;

fn capacity() -> NonZeroUsize {
    NonZeroUsize::new(2).unwrap()
}

#[test]
fn 输出序号与领域时刻无关并保持顺序() {
    let (mut producer, mut observer) = channel(capacity());
    assert_eq!(
        producer.publish("和前一帧同一 tick"),
        Push::Queued { ordinal: 1 }
    );
    assert_eq!(
        producer.publish("仍是同一 tick"),
        Push::Queued { ordinal: 2 }
    );
    assert_eq!(
        observer.poll(),
        Read::Item {
            ordinal: 1,
            value: "和前一帧同一 tick"
        }
    );
    assert_eq!(
        observer.poll(),
        Read::Item {
            ordinal: 2,
            value: "仍是同一 tick"
        }
    );
    assert_eq!(observer.poll(), Read::Empty);
}

#[test]
fn 满载时生产者不等确认_必须先获取新基线() {
    let (mut producer, mut observer) = channel(capacity());
    assert_eq!(producer.publish(1), Push::Queued { ordinal: 1 });
    assert_eq!(producer.publish(2), Push::Queued { ordinal: 2 });
    assert_eq!(producer.publish(3), Push::Gap { ordinal: 3 });
    assert_eq!(producer.publish(4), Push::Gap { ordinal: 4 });
    assert_eq!(producer.published_ordinal(), 4);
    assert_eq!(observer.poll(), Read::NeedsBaseline { through: 4 });
    assert_eq!(
        observer.resume_after(3),
        Err(ResumeError::Stale { through: 4 })
    );
    observer.resume_after(4).unwrap();
    assert_eq!(observer.poll(), Read::Empty);
    assert_eq!(producer.publish(5), Push::Queued { ordinal: 5 });
    assert_eq!(
        observer.poll(),
        Read::Item {
            ordinal: 5,
            value: 5
        }
    );
}

#[test]
fn 已跳过的观察序号不允许旧基线回退() {
    let (mut producer, mut observer) = channel(capacity());
    producer.publish(1);
    assert_eq!(
        observer.poll(),
        Read::Item {
            ordinal: 1,
            value: 1
        }
    );
    assert_eq!(
        observer.resume_after(0),
        Err(ResumeError::Stale { through: 1 })
    );
    assert_eq!(
        observer.resume_after(2),
        Err(ResumeError::Future { issued: 1 })
    );
}

#[test]
fn 与观察者断开不依赖前端回执() {
    let (mut producer, observer) = channel(capacity());
    drop(observer);
    assert_eq!(producer.publish(1), Push::ObserverClosed { ordinal: 1 });
    assert_eq!(producer.publish(2), Push::ObserverClosed { ordinal: 2 });
}

#[test]
fn 正常关闭仍消费现有前缀() {
    let (mut producer, mut observer) = channel(capacity());
    producer.publish(1);
    drop(producer);
    assert_eq!(
        observer.poll(),
        Read::Item {
            ordinal: 1,
            value: 1
        }
    );
    assert_eq!(observer.poll(), Read::Closed);
}

#[test]
fn 确认窗口只允许顺序且基线丢弃旧确认() {
    let mut credits = DeliveryCredit::new(capacity());
    assert_eq!(credits.record_sent(3), Ok(()));
    assert_eq!(credits.record_sent(4), Ok(()));
    assert!(!credits.may_send());
    assert_eq!(credits.record_sent(5), Err(DeliveryError::WindowFull));
    assert_eq!(credits.acknowledge(4), Err(DeliveryError::CursorMismatch));
    assert_eq!(credits.acknowledge(3), Ok(true));
    assert!(credits.may_send());
    credits.install_baseline(5);
    assert_eq!(credits.acknowledge(4), Ok(false));
    assert_eq!(credits.record_sent(6), Ok(()));
    assert_eq!(credits.acknowledge(6), Ok(true));
}

#[test]
fn bounded_weights_are_released_only_after_observer_take() {
    let limit = NonZeroUsize::new(4).unwrap();
    let (mut producer, mut observer) = channel_with_budget(limit, limit);
    assert_eq!(
        producer.publish_weighted("three", NonZeroUsize::new(3).unwrap()),
        Push::Queued { ordinal: 1 }
    );
    assert_eq!(
        observer.poll(),
        Read::Item {
            ordinal: 1,
            value: "three"
        }
    );
    assert_eq!(
        producer.publish_weighted("four", limit),
        Push::Queued { ordinal: 2 }
    );
    assert_eq!(producer.publish("one"), Push::Gap { ordinal: 3 });
    assert_eq!(observer.poll(), Read::NeedsBaseline { through: 3 });
    assert_eq!(observer.resume_after(3), Ok(()));
    assert_eq!(
        producer.publish_weighted("four again", limit),
        Push::Queued { ordinal: 4 }
    );
    assert_eq!(
        observer.poll(),
        Read::Item {
            ordinal: 4,
            value: "four again"
        }
    );
}

#[test]
fn indivisible_oversized_observation_excludes_all_other_queued_units() {
    let limit = NonZeroUsize::new(2).unwrap();
    let (mut producer, mut observer) = channel_with_budget(limit, limit);
    assert_eq!(
        producer.publish_weighted("huge", NonZeroUsize::new(3).unwrap()),
        Push::Queued { ordinal: 1 }
    );
    assert_eq!(producer.publish("extra"), Push::Gap { ordinal: 2 });
    assert_eq!(observer.poll(), Read::NeedsBaseline { through: 2 });
    assert_eq!(observer.resume_after(2), Ok(()));
    assert_eq!(
        producer.publish_weighted("fits", limit),
        Push::Queued { ordinal: 3 }
    );
    assert_eq!(
        observer.poll(),
        Read::Item {
            ordinal: 3,
            value: "fits"
        }
    );
}

#[test]
fn urgent_output_wakes_observer_without_overtaking_ordinary_prefix() {
    let (mut producer, mut observer) = channel(NonZeroUsize::new(3).unwrap());
    producer.publish("ordinary");
    assert!(!observer.has_urgent());
    producer.publish_weighted_urgent("terminal", NonZeroUsize::MIN);
    assert!(observer.has_urgent());
    assert_eq!(
        observer.poll(),
        Read::Item {
            ordinal: 1,
            value: "ordinary"
        }
    );
    assert!(observer.has_urgent());
    assert_eq!(
        observer.poll(),
        Read::Item {
            ordinal: 2,
            value: "terminal"
        }
    );
    assert!(!observer.has_urgent());
}

#[test]
fn exhausted_observer_window_does_not_hide_producer_gap() {
    let (mut producer, observer) = channel(NonZeroUsize::MIN);
    let mut pump = ObserverPump::new(observer, NonZeroUsize::MIN);
    assert_eq!(producer.publish("one"), Push::Queued { ordinal: 1 });
    assert_eq!(
        pump.poll(),
        PumpRead::Ready(Read::Item {
            ordinal: 1,
            value: "one"
        })
    );
    pump.record_sent(1).unwrap();
    assert_eq!(producer.publish("two"), Push::Queued { ordinal: 2 });
    assert_eq!(pump.poll(), PumpRead::CreditBlocked);
    assert_eq!(producer.publish("three"), Push::Gap { ordinal: 3 });
    assert_eq!(
        pump.poll(),
        PumpRead::Ready(Read::NeedsBaseline { through: 3 })
    );
    assert_eq!(pump.resume_after(2), Err(ResumeError::Stale { through: 3 }));
    assert_eq!(pump.resume_after(3), Ok(()));
    assert_eq!(producer.publish("four"), Push::Queued { ordinal: 4 });
    assert_eq!(
        pump.poll(),
        PumpRead::Ready(Read::Item {
            ordinal: 4,
            value: "four"
        })
    );
    pump.record_sent(4).unwrap();
    assert_eq!(pump.acknowledge(1), Ok(false));
    assert_eq!(pump.acknowledge(4), Ok(true));
}

#[test]
fn 已关闭的生产端不能越过受窗口阻挡的成功前缀() {
    let (mut producer, observer) = channel(capacity());
    let mut pump = ObserverPump::new(observer, NonZeroUsize::MIN);
    producer.publish("首包");
    producer.publish("尾包");
    drop(producer);
    assert_eq!(
        pump.poll(),
        PumpRead::Ready(Read::Item {
            ordinal: 1,
            value: "首包"
        })
    );
    pump.record_sent(1).unwrap();
    assert_eq!(pump.poll(), PumpRead::CreditBlocked);
    assert_eq!(pump.terminal_readiness(2), TerminalReadiness::Pending);
    assert_eq!(pump.acknowledge(1), Ok(true));
    assert_eq!(
        pump.poll(),
        PumpRead::Ready(Read::Item {
            ordinal: 2,
            value: "尾包"
        })
    );
    pump.record_sent(2).unwrap();
    // 终止只须排在已发送的前缀之后，不需要等待最后一个 UI ACK。
    assert_eq!(pump.terminal_readiness(2), TerminalReadiness::Ready);
    assert_eq!(pump.poll(), PumpRead::Ready(Read::Closed));
}

#[test]
fn 空轮询与生产者并发发布不能使终止通知抢跑() {
    let (mut producer, observer) = channel(capacity());
    let mut pump = ObserverPump::new(observer, NonZeroUsize::MIN);
    assert_eq!(pump.poll(), PumpRead::Ready(Read::Empty));
    producer.publish("合法前缀");
    let terminal_after = producer.published_ordinal();
    // 模拟生产者在读端返回 Empty 后交接数据并发布首错。
    assert_eq!(
        pump.terminal_readiness(terminal_after),
        TerminalReadiness::Pending
    );
    assert_eq!(
        pump.poll(),
        PumpRead::Ready(Read::Item {
            ordinal: 1,
            value: "合法前缀"
        })
    );
    // 已出队不等于已交接运输；发送者登记之后首错才能继续。
    assert_eq!(
        pump.terminal_readiness(terminal_after),
        TerminalReadiness::Pending
    );
    pump.record_sent(1).unwrap();
    assert_eq!(
        pump.terminal_readiness(terminal_after),
        TerminalReadiness::Ready
    );
}

#[test]
fn 终止通知显式报告未发送的完整观察范围() {
    let (mut producer, observer) = channel(NonZeroUsize::MIN);
    let mut pump = ObserverPump::new(observer, NonZeroUsize::MIN);
    producer.publish(1);
    assert_eq!(
        pump.poll(),
        PumpRead::Ready(Read::Item {
            ordinal: 1,
            value: 1
        })
    );
    pump.record_sent(1).unwrap();
    producer.publish(2);
    producer.publish(3);
    assert_eq!(
        pump.poll(),
        PumpRead::Ready(Read::NeedsBaseline { through: 3 })
    );
    // 缺口检测后仍可交接后续事实，终止围栏必须包含这些未送达的事实。
    producer.publish(4);
    let terminal_after = producer.published_ordinal();
    assert_eq!(
        pump.terminal_readiness(terminal_after),
        TerminalReadiness::Missing {
            from: 2,
            through: 4
        }
    );
}
