use crate::prelude::*;

/// Either an item or just its hash, to be used when inserting into a tree.
///
/// When inserting, only items inserted with [`Insert::Keep`] are retained as witnessed leaves of
/// the tree; those inserted with [`Insert::Hash`] are pruned.
#[derive(Clone, Copy, Eq, Derivative, Serialize, Deserialize)]
#[derivative(Debug)]
pub enum Insert<T> {
    /// An item unto itself: when inserting, keep this witnessed in the tree.
    Keep(T),
    /// The hash of an item: when inserting, don't keep this witnessed in the tree.
    #[derivative(Debug = "transparent")]
    Hash(Hash),
}

impl<T> Insert<T> {
    /// Transform a `&Insert<T>` into a `Insert<&T>`.
    pub fn as_ref(&self) -> Insert<&T> {
        match self {
            Insert::Keep(item) => Insert::Keep(item),
            Insert::Hash(hash) => Insert::Hash(*hash),
        }
    }

    /// Transform a `&mut Insert<T>` into a `Insert<&mut T>`.
    pub fn as_mut(&mut self) -> Insert<&mut T> {
        match self {
            Insert::Keep(item) => Insert::Keep(item),
            Insert::Hash(hash) => Insert::Hash(*hash),
        }
    }

    /// Test if this [`Insert`] is a [`Insert::Keep`].
    pub fn is_keep(&self) -> bool {
        matches!(self, Insert::Keep(_))
    }

    /// Test if this [`Insert`] is a [`Insert::Hash`].
    pub fn is_hash(&self) -> bool {
        matches!(self, Insert::Hash(_))
    }

    /// Map a function over the [`Insert::Keep`] part of an `Insert<T>`.
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Insert<U> {
        match self {
            Insert::Keep(item) => Insert::Keep(f(item)),
            Insert::Hash(hash) => Insert::Hash(hash),
        }
    }

    /// Map a function returning an `Insert<U>` over the [`Insert::Keep`] part of an `Insert<T>`.
    pub fn and_then<U, F: FnOnce(T) -> Insert<U>>(self, f: F) -> Insert<U> {
        match self {
            Insert::Keep(item) => f(item),
            Insert::Hash(hash) => Insert::Hash(hash),
        }
    }

    /// Get the kept `T` out of this [`Insert<T>`] or return `None`.
    pub fn keep(self) -> Option<T> {
        match self {
            Insert::Keep(item) => Some(item),
            Insert::Hash(_) => None,
        }
    }
}

impl<T: PartialEq<S>, S> PartialEq<Insert<S>> for Insert<T> {
    fn eq(&self, other: &Insert<S>) -> bool {
        match (self, other) {
            (Insert::Keep(item), Insert::Keep(other)) => item == other,
            (Insert::Hash(hash), Insert::Hash(other)) => hash == other,
            _ => false,
        }
    }
}

impl<T: GetHash> GetHash for Insert<T> {
    #[inline]
    fn hash(&self) -> Hash {
        match self {
            Insert::Keep(item) => item.hash(),
            Insert::Hash(hash) => *hash,
        }
    }

    #[inline]
    fn cached_hash(&self) -> Option<Hash> {
        match self {
            Insert::Keep(item) => item.cached_hash(),
            Insert::Hash(hash) => Some(*hash),
        }
    }
}

impl<T: Height> Height for Insert<T> {
    type Height = T::Height;
}

impl<T: Height + ForgetOwned> Forget for Insert<T> {
    fn forget(&mut self, forgotten: Option<Forgotten>, index: impl Into<u64>) -> bool {
        // Replace `self` temporarily with an empty hash, so we can move out of it
        let this = std::mem::replace(self, Insert::Hash(Hash::zero()));

        // Whether something was actually forgotten
        let was_forgotten;

        // No matter which branch we take, we always put something valid back into `self` before
        // returning from this function
        (*self, was_forgotten) = match this {
            Insert::Keep(item) => item.forget_owned(forgotten, index),
            Insert::Hash(_) => (this, false),
        };

        // Return whether something was actually forgotten
        was_forgotten
    }
}
