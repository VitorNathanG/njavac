//! Fast dependency-free hashing for compiler-internal maps whose iteration order
//! is not observable.

use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};

#[derive(Default)]
pub(crate) struct FxHasher {
    hash: u64,
}

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        const K: u64 = 0x51_7c_c1_b7_27_22_0a_95;
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(K);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        while bytes.len() >= 8 {
            let mut word = [0u8; 8];
            word.copy_from_slice(&bytes[..8]);
            self.add(u64::from_le_bytes(word));
            bytes = &bytes[8..];
        }
        if !bytes.is_empty() {
            let mut word = [0u8; 8];
            word[..bytes.len()].copy_from_slice(bytes);
            self.add(u64::from_le_bytes(word));
        }
    }

    #[inline]
    fn write_u8(&mut self, value: u8) {
        self.add(value as u64);
    }

    #[inline]
    fn write_u32(&mut self, value: u32) {
        self.add(value as u64);
    }

    #[inline]
    fn write_usize(&mut self, value: usize) {
        self.add(value as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

#[derive(Default, Clone)]
pub(crate) struct FxBuildHasher;

impl BuildHasher for FxBuildHasher {
    type Hasher = FxHasher;

    #[inline]
    fn build_hasher(&self) -> FxHasher {
        FxHasher::default()
    }
}

pub(crate) type FxHashMap<K, V> = HashMap<K, V, FxBuildHasher>;
