//! Native 共用的有序归档 I/O：市场不等待校验、序列化或 SQLite 事务。

use crate::{ArchiveWriter, DayEndCandidate, StoreError};
use engine::session::checkpoint::{
    ArchiveWriteFence, CapturedCheckpoint, CheckpointKey, MAX_PENDING_CHECKPOINTS,
};
use libos::native_async::{self, mpsc, oneshot};

type Job = Box<dyn FnOnce() + Send + 'static>;
type Completion = dyn Fn(CheckpointKey, Result<(), String>) + Send + Sync;

pub struct ArchiveLane {
    jobs: mpsc::Sender<Job>,
    completion: std::sync::Arc<Completion>,
    owner: libos::native_async::JoinHandle<()>,
    fence: std::sync::Arc<std::sync::Mutex<ArchiveWriteFence>>,
}

impl ArchiveLane {
    pub fn spawn(
        completion: impl Fn(CheckpointKey, Result<(), String>) + Send + Sync + 'static,
    ) -> Self {
        let (jobs, mut io) = mpsc::channel::<Job>(MAX_PENDING_CHECKPOINTS);
        let owner = native_async::spawn_io_lane(async move {
            while let Some(job) = io.recv().await {
                job();
            }
        });
        Self {
            jobs,
            completion: std::sync::Arc::new(completion),
            owner,
            fence: std::sync::Arc::new(std::sync::Mutex::new(ArchiveWriteFence::default())),
        }
    }

    pub fn stage(
        &self,
        writer: ArchiveWriter,
        checkpoint: CapturedCheckpoint,
    ) -> Result<(), StoreError> {
        let completion = self.completion.clone();
        let fence = self.fence.clone();
        let job: Job = Box::new(move || {
            let result = match fence.lock() {
                Ok(mut fence) => {
                    let result = fence.begin(checkpoint.key).and_then(|()| {
                        DayEndCandidate::from_captured(checkpoint.slot)
                            .and_then(|candidate| writer.save_day_end(&candidate))
                            .map_err(|error| error.to_string())
                    });
                    fence.finish(checkpoint.key, &result);
                    result
                }
                Err(error) => Err(format!("归档写序列锁损坏：{error}")),
            };
            completion(checkpoint.key, result);
        });
        self.jobs.try_send(job).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => {
                StoreError::Invalid("日终归档队列已满，未写入当前候选".into())
            }
            mpsc::error::TrySendError::Closed(_) => {
                StoreError::Invalid("日终归档任务已退出".into())
            }
        })
    }

    /// 只交接查询并返回完成句柄；调用方可将等待移出市场任务。
    pub fn try_execute<T: Send + 'static>(
        &self,
        action: impl FnOnce() -> Result<T, StoreError> + Send + 'static,
    ) -> Result<oneshot::Receiver<Result<T, StoreError>>, StoreError> {
        let (reply, result) = oneshot::channel();
        let job: Job = Box::new(move || {
            let _ = reply.send(action());
        });
        self.jobs.try_send(job).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => {
                StoreError::Invalid("日终归档队列已满，读档未入队".into())
            }
            mpsc::error::TrySendError::Closed(_) => {
                StoreError::Invalid("日终归档任务已退出".into())
            }
        })?;
        Ok(result)
    }

    /// 显式读档、换代在同一队列越过已有的写任务。市场推进不经过此调用。
    pub async fn execute<T: Send + 'static>(
        &self,
        action: impl FnOnce() -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError> {
        let (reply, result) = oneshot::channel();
        self.jobs
            .send(Box::new(move || {
                let _ = reply.send(action());
            }))
            .await
            .map_err(|_| StoreError::Invalid("存档任务已退出".into()))?;
        result
            .await
            .map_err(|_| StoreError::Invalid("存档任务没有返回结果".into()))?
    }

    pub async fn finish(self) -> Result<(), StoreError> {
        drop(self.jobs);
        self.owner
            .await
            .map_err(|error| StoreError::Invalid(format!("存档任务异常退出：{error}")))?;
        let result = self
            .fence
            .lock()
            .map_err(|error| StoreError::Invalid(format!("归档写序列锁损坏：{error}")))?
            .ensure_finished()
            .map_err(StoreError::Invalid);
        result
    }
}

#[cfg(test)]
#[path = "../../engine/tests/publications/session_fixture.rs"]
mod fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NativeDatabase;
    use engine::session::protocol::ProtocolSession;

    #[test]
    fn 停止发布后归档在独立队列按序写入并可加载() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .build()
            .unwrap();
        runtime.block_on(async {
            let day = engine::CivilDate::from_iso("2030-01-05").unwrap();
            let mut game = ProtocolSession::new(fixture::civil_setup(day), 42).unwrap();
            let db = NativeDatabase::open_in_memory().unwrap();
            let writer = ArchiveWriter::activate(db.clone(), "current").unwrap();
            let (receipts, mut completed) = mpsc::unbounded_channel();
            let lane = ArchiveLane::spawn(move |key, result| {
                let _ = receipts.send((key, result));
            });
            let (started, ready) = oneshot::channel();
            let (release, blocked) = std::sync::mpsc::channel::<()>();
            lane.jobs
                .try_send(Box::new(move || {
                    let _ = started.send(());
                    let _ = blocked.recv_timeout(std::time::Duration::from_secs(2));
                }))
                .ok()
                .unwrap();
            ready.await.unwrap();
            for _ in 0..2 {
                let civil = game.end_civil_day_update().unwrap();
                let checkpoint = CapturedCheckpoint::take(&game, &civil, 1, 1).unwrap();
                lane.stage(writer.clone(), checkpoint).unwrap();
            }
            assert!(db.load("current").unwrap().is_none());
            assert!(completed.try_recv().is_err());
            release.send(()).unwrap();
            let saved_db = db.clone();
            let last = lane
                .execute(move || saved_db.load("current"))
                .await
                .unwrap()
                .unwrap();
            let (first, first_result) = completed.try_recv().unwrap();
            let (second, second_result) = completed.try_recv().unwrap();
            assert!(first_result.is_ok() && second_result.is_ok());
            assert!(first.seq < second.seq);
            assert_eq!(last.snapshot.seq, second.seq);
            assert_eq!(last.civil_clock.settled_through, Some(second.settled_date));
            lane.finish().await.unwrap();
        });
    }
    #[test]
    fn 首个写失败后较晚候选不越过漏档且关闭报未耐久() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .build()
            .unwrap();
        runtime.block_on(async {
            let day = engine::CivilDate::from_iso("2030-01-05").unwrap();
            let mut game = ProtocolSession::new(fixture::civil_setup(day), 42).unwrap();
            let db = NativeDatabase::open_in_memory().unwrap();
            let stale_writer = ArchiveWriter::activate(db.clone(), "current").unwrap();
            let current_writer = stale_writer.renew(None).unwrap();
            let (receipts, mut completed) = mpsc::unbounded_channel();
            let lane = ArchiveLane::spawn(move |key, result| {
                let _ = receipts.send((key, result));
            });
            let civil = game.end_civil_day_update().unwrap();
            lane.stage(
                stale_writer,
                CapturedCheckpoint::take(&game, &civil, 1, 1).unwrap(),
            )
            .unwrap();
            let civil = game.end_civil_day_update().unwrap();
            lane.stage(
                current_writer,
                CapturedCheckpoint::take(&game, &civil, 1, 1).unwrap(),
            )
            .unwrap();
            assert!(lane.finish().await.is_err());
            assert!(completed.try_recv().unwrap().1.is_err());
            assert!(completed
                .try_recv()
                .unwrap()
                .1
                .unwrap_err()
                .contains("blocked"));
            assert!(db.load("current").unwrap().is_none());
        });
    }
}
