//! Append-only personal facts shared across tick candidates without copying old years.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::ops::Index;
use std::sync::Arc;

const CHUNK_CAPACITY: usize = 32;

struct HistoryChunk<T> {
    previous: Option<Arc<HistoryChunk<T>>>,
    values: Vec<T>,
}

/// Empty histories allocate nothing. A clone shares the tail; adding a fact
/// copies at most one bounded tail chunk and never copies older chunks.
pub struct AppendOnlyHistory<T> {
    tail: Option<Arc<HistoryChunk<T>>>,
    len: usize,
}

impl<T> Default for AppendOnlyHistory<T> {
    fn default() -> Self {
        Self { tail: None, len: 0 }
    }
}

impl<T> Clone for AppendOnlyHistory<T> {
    fn clone(&self) -> Self {
        Self {
            tail: self.tail.clone(),
            len: self.len,
        }
    }
}

impl<T> Drop for AppendOnlyHistory<T> {
    fn drop(&mut self) {
        let mut tail = self.tail.take();
        while let Some(chunk) = tail {
            // Arc::into_inner 原子地检查唯一所有权：两个并发最后 owner 竞争时
            // 恰好一方得到 Some、另一方得到 None——不会像 try_unwrap 双 Err
            // 那样让最后 Err 的 Arc 递归释放 previous 链（长历史 stack overflow）。
            let Some(mut owned) = Arc::into_inner(chunk) else {
                break;
            };
            tail = owned.previous.take();
        }
    }
}

impl<T> AppendOnlyHistory<T> {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn last(&self) -> Option<&T> {
        self.tail.as_ref()?.values.last()
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.len {
            return None;
        }
        let mut from_end = self.len - index - 1;
        let mut chunk = self.tail.as_deref();
        while let Some(current) = chunk {
            if from_end < current.values.len() {
                return current.values.get(current.values.len() - from_end - 1);
            }
            from_end -= current.values.len();
            chunk = current.previous.as_deref();
        }
        None
    }

    pub fn iter(&self) -> HistoryIter<'_, T> {
        let mut chunks = Vec::new();
        let mut current = self.tail.as_deref();
        while let Some(chunk) = current {
            chunks.push(chunk);
            current = chunk.previous.as_deref();
        }
        chunks.reverse();
        HistoryIter {
            chunks,
            chunk_index: 0,
            value_index: 0,
            remaining: self.len,
        }
    }

    /// Walk newest facts first without building an index over the full history.
    pub fn iter_rev(&self) -> HistoryRevIter<'_, T> {
        HistoryRevIter {
            chunk: self.tail.as_deref(),
            next_index: self.tail.as_ref().map_or(0, |chunk| chunk.values.len()),
            remaining: self.len,
        }
    }
}

impl<T: Clone> AppendOnlyHistory<T> {
    pub fn push(&mut self, value: T) {
        if let Some(tail) = self.tail.as_mut() {
            if tail.values.len() < CHUNK_CAPACITY {
                if let Some(unique) = Arc::get_mut(tail) {
                    unique.values.push(value);
                    self.len += 1;
                    return;
                }
            }
        }
        let next = match self.tail.as_ref() {
            None => Arc::new(HistoryChunk {
                previous: None,
                values: vec![value],
            }),
            Some(tail) if tail.values.len() < CHUNK_CAPACITY => {
                let mut values = tail.values.clone();
                values.push(value);
                Arc::new(HistoryChunk {
                    previous: tail.previous.clone(),
                    values,
                })
            }
            Some(tail) => Arc::new(HistoryChunk {
                previous: Some(tail.clone()),
                values: vec![value],
            }),
        };
        self.tail = Some(next);
        self.len += 1;
    }

    /// 弹出最后一个值（同事务回退用）。COW：只复制当前尾 chunk（有界），
    /// 更早 chunk 与旧快照继续共享；空历史返回 `None`。
    pub fn pop(&mut self) -> Option<T> {
        let mut tail = self.tail.take()?;
        let last = tail.values.last().cloned();
        let replacement = if tail.values.len() > 1 {
            match Arc::get_mut(&mut tail) {
                Some(unique) => {
                    unique.values.pop();
                    Some(tail)
                }
                None => {
                    let mut values = tail.values.clone();
                    values.pop();
                    Some(Arc::new(HistoryChunk {
                        previous: tail.previous.clone(),
                        values,
                    }))
                }
            }
        } else {
            // 尾 chunk 只剩这一个值：tail 回退到 previous chunk（共享，不复制）；
            // 首个 chunk（previous=None）弹空后整条历史回到空。
            tail.previous.clone()
        };
        self.tail = replacement;
        self.len -= 1;
        last
    }
}

pub struct HistoryIter<'a, T> {
    chunks: Vec<&'a HistoryChunk<T>>,
    chunk_index: usize,
    value_index: usize,
    remaining: usize,
}

impl<'a, T> Iterator for HistoryIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let chunk = self.chunks.get(self.chunk_index)?;
            if let Some(value) = chunk.values.get(self.value_index) {
                self.value_index += 1;
                self.remaining -= 1;
                return Some(value);
            }
            self.chunk_index += 1;
            self.value_index = 0;
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<T> ExactSizeIterator for HistoryIter<'_, T> {}

pub struct HistoryRevIter<'a, T> {
    chunk: Option<&'a HistoryChunk<T>>,
    next_index: usize,
    remaining: usize,
}

impl<'a, T> Iterator for HistoryRevIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let chunk = self.chunk?;
            if self.next_index > 0 {
                self.next_index -= 1;
                self.remaining -= 1;
                return chunk.values.get(self.next_index);
            }
            self.chunk = chunk.previous.as_deref();
            self.next_index = self.chunk.map_or(0, |previous| previous.values.len());
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<T> ExactSizeIterator for HistoryRevIter<'_, T> {}

impl<'a, T> IntoIterator for &'a AppendOnlyHistory<T> {
    type Item = &'a T;
    type IntoIter = HistoryIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T> Index<usize> for AppendOnlyHistory<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).expect("history index is out of bounds")
    }
}

impl<T: PartialEq> PartialEq for AppendOnlyHistory<T> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len
            && (match (&self.tail, &other.tail) {
                (Some(left), Some(right)) if Arc::ptr_eq(left, right) => true,
                _ => self.iter().eq(other.iter()),
            })
    }
}

impl<T: Eq> Eq for AppendOnlyHistory<T> {}

impl<T: fmt::Debug> fmt::Debug for AppendOnlyHistory<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: Serialize> Serialize for AppendOnlyHistory<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}

impl<'de, T: Deserialize<'de> + Clone> Deserialize<'de> for AppendOnlyHistory<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut history = Self::default();
        for value in Vec::<T>::deserialize(deserializer)? {
            history.push(value);
        }
        Ok(history)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releasing_a_long_shared_history_does_not_recurse_through_chunks() {
        let mut history = AppendOnlyHistory::default();
        for value in 0..327_680_u32 {
            history.push(value);
        }
        let candidate = history.clone();
        drop(history);
        assert_eq!(candidate.len(), 327_680);
        assert_eq!(candidate.last(), Some(&327_679));
        drop(candidate);
    }

    #[test]
    fn pop_returns_last_value_and_keeps_older_facts_shared() {
        let mut history = AppendOnlyHistory::default();
        for value in 0..70_u32 {
            history.push(value);
        }
        let snapshot = history.clone();
        assert_eq!(history.pop(), Some(69));
        assert_eq!(history.len(), 69);
        assert_eq!(history.last(), Some(&68));
        // 旧快照不受 pop 影响；pop 后双方仍共享全部更早 chunk。
        assert_eq!(snapshot.len(), 70);
        assert_eq!(snapshot.last(), Some(&69));
        // 值语义一致（ptr 快路径或逐项相等均可）。
        let mut rebuilt = AppendOnlyHistory::default();
        for value in 0..69_u32 {
            rebuilt.push(value);
        }
        assert_eq!(history, rebuilt);
    }

    #[test]
    fn pop_on_chunk_boundary_reuses_previous_chunk() {
        let mut history = AppendOnlyHistory::default();
        for value in 0..33_u32 {
            history.push(value);
        }
        // 33 = 32 + 1：pop 掉第二 chunk 的唯一元素后，tail 应回到第一 chunk。
        assert_eq!(history.pop(), Some(32));
        assert_eq!(history.len(), 32);
        assert_eq!(history.last(), Some(&31));
        assert_eq!(history.pop(), Some(31));
        // 连续 pop 到空。
        while history.pop().is_some() {}
        assert!(history.is_empty());
        assert_eq!(history.pop(), None);
    }

    #[test]
    fn interleaved_push_pop_keeps_snapshot_isolation() {
        let mut history = AppendOnlyHistory::default();
        for value in 0..40_u32 {
            history.push(value);
        }
        let snapshot = history.clone();
        history.pop();
        history.push(100);
        history.pop();
        history.push(101);
        assert_eq!(history.len(), 40);
        assert_eq!(history.last(), Some(&101));
        assert_eq!(snapshot.len(), 40);
        assert_eq!(snapshot.last(), Some(&39));
        let snapshot_values: Vec<_> = snapshot.iter().copied().collect();
        assert_eq!(snapshot_values, (0..40).collect::<Vec<_>>());
    }

    /// 两个线程同时 drop 各自的最后 clone：Arc::into_inner 保证恰好一方
    /// 得到 Some 并迭代断链，另一方得到 None 并退出——不会出现
    /// try_unwrap 双 Err 后最后 Err 的 Arc 递归释放 previous 链的爆栈。
    #[test]
    fn concurrent_last_owner_drop_does_not_recursively_release_chunk_chain() {
        use std::sync::{Arc, Barrier};
        let mut history = AppendOnlyHistory::default();
        // 327680 / 32 = 10240 chunks——足够深的链，递归释放会 stack overflow。
        for value in 0..327_680_u32 {
            history.push(value);
        }
        let clone = history.clone();
        let barrier = Arc::new(Barrier::new(2));
        let thread_barrier = barrier.clone();
        let other = std::thread::spawn(move || {
            thread_barrier.wait();
            drop(clone);
        });
        barrier.wait();
        drop(history);
        other.join().expect("并发 drop 线程 panic");
    }
}
