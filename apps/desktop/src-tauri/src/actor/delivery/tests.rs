use super::*;
use std::sync::mpsc;
use std::time::Duration;

async fn consume_without_terminal<T, F>(
    observer: Observer<T>,
    send: F,
    signal: Arc<WakeEpoch>,
    controlled: tokio::sync::mpsc::Receiver<Control>,
    window: NonZeroUsize,
) where
    F: FnMut(DeliveryAction<T>) -> bool,
{
    consume(
        observer,
        send,
        signal,
        controlled,
        window,
        Arc::new(OnceLock::new()),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocked_delivery_does_not_wait_for_frontend_or_stop_authority() {
    let (mut producer, observer) = publication::channel::<u64>(NonZeroUsize::MIN);
    let (captured, captured_rx) = tokio::sync::mpsc::unbounded_channel();
    let (release, release_rx) = mpsc::channel::<()>();
    let sink = libos::native_async::spawn_io_lane(async move {
        let signal = Arc::clone(observer.signal());
        let (_, resume) = tokio::sync::mpsc::channel(1);
        consume_without_terminal(
            observer,
            move |action| match action {
                DeliveryAction::Item { value: item, .. } => {
                    captured.send(Some(item)).unwrap();
                    let _ = release_rx.recv_timeout(Duration::from_secs(1));
                    true
                }
                DeliveryAction::Terminal(_) => panic!("nonterminal fixture"),
                DeliveryAction::Gap(through) => {
                    captured.send(None).unwrap();
                    assert_eq!(through, 3);
                    false
                }
            },
            signal,
            resume,
            NonZeroUsize::new(2).unwrap(),
        )
        .await;
    });
    let mut captured_rx = captured_rx;
    assert_eq!(
        producer.publish(1),
        publication::Push::Queued { ordinal: 1 }
    );
    assert_eq!(captured_rx.recv().await, Some(Some(1)));

    assert_eq!(
        producer.publish(2),
        publication::Push::Queued { ordinal: 2 }
    );
    assert_eq!(producer.publish(3), publication::Push::Gap { ordinal: 3 });
    release.send(()).unwrap();

    assert_eq!(captured_rx.recv().await, Some(None));
    sink.await.unwrap();
}

#[tokio::test]
async fn observer_fence_resumes_only_after_the_captured_gap() {
    let (mut producer, observer) = publication::channel::<u64>(NonZeroUsize::MIN);
    assert_eq!(
        producer.publish(1),
        publication::Push::Queued { ordinal: 1 }
    );
    assert_eq!(producer.publish(2), publication::Push::Gap { ordinal: 2 });
    let signal = Arc::clone(observer.signal());
    let producer_signal = Arc::clone(&signal);
    let (resume, requests) = tokio::sync::mpsc::channel(1);
    let (captured, mut output) = tokio::sync::mpsc::unbounded_channel();
    let sink = libos::native_async::spawn_io_lane(consume_without_terminal(
        observer,
        move |event| {
            captured.send(event).unwrap();
            true
        },
        signal,
        requests,
        NonZeroUsize::new(2).unwrap(),
    ));
    assert!(matches!(output.recv().await, Some(DeliveryAction::Gap(2))));

    let (reply, ack) = tokio::sync::oneshot::channel();
    resume
        .try_send(Control::Resume(ResumeRequest { ordinal: 1, reply }))
        .unwrap_or_else(|_| panic!("恢复请求应入队"));
    producer_signal.notify();
    assert_eq!(
        ack.await.unwrap(),
        Err("OBSERVER_BASELINE_STALE: 2".to_owned())
    );
    let (reply, ack) = tokio::sync::oneshot::channel();
    resume
        .try_send(Control::Resume(ResumeRequest { ordinal: 2, reply }))
        .unwrap_or_else(|_| panic!("恢复请求应入队"));
    producer_signal.notify();
    assert_eq!(ack.await.unwrap(), Ok(()));

    assert_eq!(
        producer.publish(3),
        publication::Push::Queued { ordinal: 3 }
    );
    assert!(matches!(
        output.recv().await,
        Some(DeliveryAction::Item {
            ordinal: 3,
            value: 3
        })
    ));
    drop(producer);
    producer_signal.close();
    sink.await.unwrap();
}

#[tokio::test]
async fn renderer_credit_caps_only_observer_delivery() {
    let (mut producer, observer) = publication::channel::<u64>(NonZeroUsize::new(4).unwrap());
    let signal = Arc::clone(observer.signal());
    let ack_signal = Arc::clone(&signal);
    let (commands, controlled) = tokio::sync::mpsc::channel(4);
    let (captured, mut output) = tokio::sync::mpsc::unbounded_channel();
    let sink = libos::native_async::spawn_io_lane(consume_without_terminal(
        observer,
        move |event| {
            captured.send(event).unwrap();
            true
        },
        signal,
        controlled,
        NonZeroUsize::MIN,
    ));
    assert_eq!(
        producer.publish(1),
        publication::Push::Queued { ordinal: 1 }
    );
    assert!(matches!(
        output.recv().await,
        Some(DeliveryAction::Item {
            ordinal: 1,
            value: 1
        })
    ));
    assert_eq!(
        producer.publish(2),
        publication::Push::Queued { ordinal: 2 }
    );
    assert_eq!(
        producer.publish(3),
        publication::Push::Queued { ordinal: 3 }
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(20), output.recv())
            .await
            .is_err()
    );

    let (reply, result) = tokio::sync::oneshot::channel();
    assert!(commands
        .try_send(Control::Ack(AckRequest { ordinal: 1, reply }))
        .is_ok());
    ack_signal.notify();
    assert_eq!(result.await.unwrap(), Ok(()));
    assert!(matches!(
        output.recv().await,
        Some(DeliveryAction::Item {
            ordinal: 2,
            value: 2
        })
    ));

    drop(producer);
    // 生产者关闭不是队列排空；ACK 路由还在时允许最后的成功帧发送。
    let (reply, result) = tokio::sync::oneshot::channel();
    assert!(commands
        .try_send(Control::Ack(AckRequest { ordinal: 2, reply }))
        .is_ok());
    ack_signal.notify();
    assert_eq!(result.await.unwrap(), Ok(()));
    assert!(matches!(
        output.recv().await,
        Some(DeliveryAction::Item {
            ordinal: 3,
            value: 3,
        })
    ));
    sink.await.unwrap();
}

#[tokio::test]
async fn observer_accepts_fence_while_held_without_delivering_next_packet() {
    let (mut producer, observer) = publication::channel::<u64>(NonZeroUsize::new(2).unwrap());
    let signal = Arc::clone(observer.signal());
    let (control, requests) = tokio::sync::mpsc::channel(2);
    let (captured, mut output) = tokio::sync::mpsc::unbounded_channel();
    let sink = libos::native_async::spawn_io_lane(consume_without_terminal(
        observer,
        move |event| {
            captured.send(event).unwrap();
            false
        },
        Arc::clone(&signal),
        requests,
        NonZeroUsize::new(2).unwrap(),
    ));

    let (reply, ack) = oneshot::channel();
    control
        .send(Control::Hold { held: true, reply })
        .await
        .unwrap_or_else(|_| panic!("hold unavailable"));
    signal.notify();
    ack.await.unwrap().unwrap();
    producer.publish(1);

    let (reply, ack) = oneshot::channel();
    control
        .send(Control::Resume(ResumeRequest { ordinal: 0, reply }))
        .await
        .unwrap_or_else(|_| panic!("resume unavailable"));
    signal.notify();
    ack.await.unwrap().unwrap();
    assert!(
        output.try_recv().is_err(),
        "accepting the cut must not release queued packets"
    );

    let (reply, ack) = oneshot::channel();
    control
        .send(Control::Hold { held: false, reply })
        .await
        .unwrap_or_else(|_| panic!("release unavailable"));
    signal.notify();
    ack.await.unwrap().unwrap();
    assert!(matches!(
        output.recv().await,
        Some(DeliveryAction::Item {
            ordinal: 1,
            value: 1
        })
    ));
    sink.await.unwrap();
}

#[tokio::test]
async fn terminal_survives_queue_overflow_closed_credit_and_fenced_hold() {
    for (capacity, count, held, expect_prefix, missing) in [
        (1, 2, false, false, Some("1–2")),
        (2, 2, false, true, Some("2–2")),
        (2, 1, false, true, None),
        (2, 1, true, false, Some("1–1")),
    ] {
        let (mut producer, observer) =
            publication::channel::<u64>(NonZeroUsize::new(capacity).unwrap());
        let signal = Arc::clone(observer.signal());
        let (control, requests) = tokio::sync::mpsc::channel(1);
        if held {
            let (reply, _) = oneshot::channel();
            assert!(control
                .try_send(Control::Hold { held: true, reply })
                .is_ok());
        }
        for value in 1..=count {
            producer.publish(value);
        }
        let record = Arc::new(OnceLock::new());
        assert!(record
            .set(Terminal {
                after: producer.published_ordinal(),
                failure: EngineFailurePayload::terminal_fixture()
            })
            .is_ok());
        drop(producer);
        drop(control);
        let mut delivered = Vec::new();
        consume(
            observer,
            |event| {
                delivered.push(event);
                true
            },
            signal,
            requests,
            NonZeroUsize::MIN,
            record,
        )
        .await;
        let terminal = delivered
            .pop()
            .expect("first authority failure must survive");
        let DeliveryAction::Terminal(payload) = terminal else {
            panic!("expected original terminal as the final action");
        };
        let wire = serde_json::to_value(payload).unwrap();
        assert_eq!(wire["code"], "TEST_TERMINAL");
        assert!(
            wire.get("publication_ordinal").is_none(),
            "terminal never consumes credit"
        );
        let message = wire["message"].as_str().unwrap();
        match missing {
            Some(range) => assert!(
                message.contains(range),
                "{message} did not include missing {range}"
            ),
            None => assert!(
                !message.contains("观察者未收到"),
                "{message} falsely reports a gap"
            ),
        }
        if expect_prefix {
            assert!(matches!(
                &delivered[..],
                [DeliveryAction::Item {
                    ordinal: 1,
                    value: 1
                }]
            ));
        } else {
            assert!(
                delivered.is_empty(),
                "no packet can overtake the terminal fence"
            );
        }
    }
}
