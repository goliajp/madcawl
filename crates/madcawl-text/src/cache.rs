//! The shaping cache: shaped words by (text, face, size, features, direction).

use crate::font::FaceId;
use crate::lru::{FxBuild, Lru};
use crate::shape::Glyph;
use std::hash::{BuildHasher, Hash, Hasher};
use std::sync::Arc;

/// A shaped word: its glyphs in visual order and their total advance in pixels.
#[derive(Debug, PartialEq)]
pub struct Shaped {
    pub glyphs: Box<[Glyph]>,
    pub advance: f32,
    pub rtl: bool,
}

struct Key {
    text: Box<[u16]>,
    face: FaceId,
    size: u32,
    features: u32,
    rtl: bool,
}

/// Counters of a [`ShapeCache`].
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub entries: usize,
    /// The weight held: glyphs plus code units over all entries.
    pub weight: usize,
}

impl CacheStats {
    /// Hits over lookups; 0 before the first lookup.
    pub fn hit_rate(&self) -> f64 {
        let n = self.hits + self.misses;
        if n == 0 {
            0.0
        } else {
            self.hits as f64 / n as f64
        }
    }
}

/// Shaped words, bounded by entry count and by total glyphs plus code units; full, it evicts the least recently
/// used word, one at a time. Entries are keyed by a 64-bit hash of the whole key and the key is compared on
/// every hit, so a hash collision is a miss, never a wrong result.
pub struct ShapeCache {
    lru: Lru<u64, (Key, Arc<Shaped>)>,
    hits: u64,
    misses: u64,
}

fn hash(text: &[u16], face: FaceId, size: u32, features: u32, rtl: bool) -> u64 {
    let mut h = FxBuild::default().build_hasher();
    text.hash(&mut h);
    (face, size, features, rtl).hash(&mut h);
    h.finish()
}

impl ShapeCache {
    pub fn new(max_entries: usize, max_weight: usize) -> Self {
        ShapeCache {
            lru: Lru::new(max_entries, max_weight),
            hits: 0,
            misses: 0,
        }
    }

    pub(crate) fn get(
        &mut self,
        text: &[u16],
        face: FaceId,
        size: f32,
        features: u32,
        rtl: bool,
    ) -> Option<Arc<Shaped>> {
        let size = size.to_bits();
        let h = hash(text, face, size, features, rtl);
        let hit = match self.lru.get(&h) {
            Some((k, v))
                if *k.text == *text
                    && k.face == face
                    && k.size == size
                    && k.features == features
                    && k.rtl == rtl =>
            {
                Some(v.clone())
            }
            _ => None,
        };
        if hit.is_some() {
            self.hits += 1
        } else {
            self.misses += 1
        }
        hit
    }

    pub(crate) fn insert(
        &mut self,
        text: &[u16],
        face: FaceId,
        size: f32,
        features: u32,
        shaped: Arc<Shaped>,
    ) {
        let rtl = shaped.rtl;
        let size = size.to_bits();
        let weight = shaped.glyphs.len() + text.len();
        let key = Key {
            text: text.into(),
            face,
            size,
            features,
            rtl,
        };
        self.lru
            .insert(hash(text, face, size, features, rtl), (key, shaped), weight);
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits,
            misses: self.misses,
            evictions: self.lru.evictions,
            entries: self.lru.len(),
            weight: self.lru.weight(),
        }
    }
}
