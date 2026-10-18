//! ledger 行共享不可变 map，所有可变访问通过 Arc::make_mut 隔离。
//! 审计缓存只保留 Weak identity；唯一 owner 的 mutation 不会因缓存而深拷贝。
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Weak};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SharedRows<T>(Arc<T>);

impl<T> From<T> for SharedRows<T> {
    fn from(value: T) -> Self {
        Self(Arc::new(value))
    }
}

impl<T: Default> Default for SharedRows<T> {
    fn default() -> Self {
        Self::from(T::default())
    }
}

impl<T> Deref for SharedRows<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: Clone> DerefMut for SharedRows<T> {
    fn deref_mut(&mut self) -> &mut T {
        Arc::make_mut(&mut self.0)
    }
}

impl<T> SharedRows<T> {
    pub(super) fn identity(&self) -> Weak<T> {
        Arc::downgrade(&self.0)
    }
    pub(super) fn matches(&self, identity: &Weak<T>) -> bool {
        std::ptr::eq(Arc::as_ptr(&self.0), identity.as_ptr())
    }
}

impl<T: serde::Serialize> serde::Serialize for SharedRows<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_ref().serialize(serializer)
    }
}

impl<T: Clone + IntoIterator> IntoIterator for SharedRows<T> {
    type Item = T::Item;
    type IntoIter = T::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        Arc::unwrap_or_clone(self.0).into_iter()
    }
}

impl<'a, T> IntoIterator for &'a SharedRows<T>
where
    &'a T: IntoIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = <&'a T as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        self.0.as_ref().into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Counted {
        clones: Arc<AtomicUsize>,
        value: u64,
    }
    impl Clone for Counted {
        fn clone(&self) -> Self {
            self.clones.fetch_add(1, Ordering::Relaxed);
            Self {
                clones: self.clones.clone(),
                value: self.value,
            }
        }
    }

    #[test]
    fn unique_owner_mutation_invalidates_weak_identity_without_cloning_value() {
        let clones = Arc::new(AtomicUsize::new(0));
        let mut rows = SharedRows::from(Counted {
            clones: clones.clone(),
            value: 7,
        });
        let identity = rows.identity();
        rows.value = 8;
        assert!(!rows.matches(&identity));
        assert_eq!(clones.load(Ordering::Relaxed), 0);
        assert_eq!(rows.value, 8);
    }

    #[test]
    fn shared_owner_mutation_clones_once_and_keeps_old_value_and_serialization() {
        let authority = SharedRows::from(vec![1, 2, 3]);
        let identity = authority.identity();
        let mut candidate = authority.clone();
        candidate.push(4);
        assert!(authority.matches(&identity));
        assert!(!candidate.matches(&identity));
        assert_eq!(serde_json::to_string(&authority).unwrap(), "[1,2,3]");
        assert_eq!(serde_json::to_string(&candidate).unwrap(), "[1,2,3,4]");
    }
}
