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
            // Another history still owns this suffix. Its eventual owner will
            // release the chain iteratively when the final reference goes away.
            let Ok(mut owned) = Arc::try_unwrap(chunk) else {
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
}
