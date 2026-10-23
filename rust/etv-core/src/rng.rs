//! Bit-exact CPython `random.Random` & `SeededRNG` (`models.py`, `save.py`).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

pub const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_B0DF;
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7FFF_FFFF;

/// Serialized representation matching CPython `random.Random.getstate()` in `.trail/run.json`:
/// `[3, [w0, ..., w623, index], gauss_next]`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RngState(pub u32, pub Vec<u32>, pub Option<f64>);

/// Alias kept for Spike S1 compatibility.
pub type RngStateTuple = RngState;

/// Custom serde deserializer for `Option<RngState>` that uses [`coerce_rng_state`].
/// Any malformed or non-v3 `rng_state` in JSON deserializes to `None` instead of failing.
pub fn deserialize_lenient_rng_state<'de, D>(deserializer: D) -> Result<Option<RngState>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let val = Value::deserialize(deserializer)?;
    Ok(coerce_rng_state(&val))
}

/// Leniently parse `rng_state` from a `serde_json::Value`, matching `save.py::_coerce_rng_state`.
///
/// Validates:
/// - outer shape is a 3-element array `[version, internal, gauss_next]`
/// - `version == 3` (integer)
/// - `internal` is an array of 625 integers (`u32`, where `internal[624] <= 624`)
/// - `gauss_next` is `null` or a finite float/integer
///
/// Anything else returns `None` without failing the load.
pub fn coerce_rng_state(raw: &Value) -> Option<RngState> {
    let arr = raw.as_array()?;
    if arr.len() != 3 {
        return None;
    }
    let version = arr[0].as_u64()?;
    if version != 3 {
        return None;
    }
    let internal_arr = arr[1].as_array()?;
    if internal_arr.len() != N + 1 {
        return None;
    }
    let mut internal = Vec::with_capacity(N + 1);
    for v in internal_arr {
        // Must be an integer (not bool, float, or string) fitting in u32
        let word_u64 = v.as_u64()?;
        let word = u32::try_from(word_u64).ok()?;
        internal.push(word);
    }
    if (internal[N] as usize) > N {
        return None;
    }
    let gauss_next = if arr[2].is_null() {
        None
    } else {
        Some(arr[2].as_f64()?)
    };
    Some(RngState(3, internal, gauss_next))
}

/// Bit-exact port of CPython's Mersenne Twister (`_randommodule.c` + `Lib/random.py`)
/// and Escape the Valley's `SeededRNG` wrapper (`models.py`).
#[derive(Debug, Clone, PartialEq)]
pub struct CpythonRng {
    mt: [u32; N],
    index: usize,
    gauss_next: Option<f64>,
    pub counter: u64,
}

impl CpythonRng {
    /// Seed from any signed 128-bit integer using CPython's `random_seed` algorithm:
    /// take `abs(seed)`, split into 32-bit little-endian unsigned words, and call `init_by_array`.
    pub fn from_seed(seed: i128) -> Self {
        let mut rng = Self {
            mt: [0u32; N],
            index: N + 1,
            gauss_next: None,
            counter: 0,
        };
        let key = Self::seed_to_key(seed);
        rng.init_by_array(&key);
        rng
    }

    /// Construct from `seed`, replay `random()` `counter` times (matching `SeededRNG(seed, counter)`
    /// in `models.py:364-370`), set `self.counter = counter`, and then apply `rng_state` if
    /// present and valid (matching `engine.py:92-107` and `step_engine.py:217-227`).
    /// If `rng_state` is invalid, keeps the counter-replayed state.
    pub fn from_seed_and_counter(seed: i128, counter: u64, rng_state: Option<&RngState>) -> Self {
        let mut rng = Self::from_seed(seed);
        for _ in 0..counter {
            rng.raw_random();
        }
        rng.counter = counter;
        if let Some(state) = rng_state {
            let _ = rng.setstate(state);
        }
        rng
    }

    /// Convert a signed integer into CPython's little-endian `u32` word array.
    pub fn seed_to_key(seed: i128) -> Vec<u32> {
        let mut rem = seed.unsigned_abs();
        if rem == 0 {
            return vec![0u32];
        }
        let mut key = Vec::new();
        while rem > 0 {
            key.push((rem & 0xFFFF_FFFF) as u32);
            rem >>= 32;
        }
        key
    }

    fn init_genrand(&mut self, s: u32) {
        self.mt[0] = s;
        for mti in 1..N {
            self.mt[mti] = 1812433253u32
                .wrapping_mul(self.mt[mti - 1] ^ (self.mt[mti - 1] >> 30))
                .wrapping_add(mti as u32);
        }
        self.index = N;
    }

    pub fn init_by_array(&mut self, init_key: &[u32]) {
        let key_length = init_key.len();
        self.init_genrand(19650218u32);
        let mut i: usize = 1;
        let mut j: usize = 0;
        let mut k: usize = if N > key_length { N } else { key_length };
        while k != 0 {
            self.mt[i] = (self.mt[i]
                ^ ((self.mt[i - 1] ^ (self.mt[i - 1] >> 30)).wrapping_mul(1664525u32)))
            .wrapping_add(init_key[j])
            .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            if j >= key_length {
                j = 0;
            }
            k -= 1;
        }
        k = N - 1;
        while k != 0 {
            self.mt[i] = (self.mt[i]
                ^ ((self.mt[i - 1] ^ (self.mt[i - 1] >> 30)).wrapping_mul(1566083941u32)))
            .wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            k -= 1;
        }
        self.mt[0] = 0x8000_0000u32;
        self.gauss_next = None;
    }

    /// Generate one tempered 32-bit word (`genrand_uint32` in `_randommodule.c`).
    pub fn genrand_uint32(&mut self) -> u32 {
        let mut y: u32;
        const MAG01: [u32; 2] = [0x0, MATRIX_A];
        if self.index >= N {
            for kk in 0..(N - M) {
                y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
                self.mt[kk] = self.mt[kk + M] ^ (y >> 1) ^ MAG01[(y & 0x1) as usize];
            }
            for kk in (N - M)..(N - 1) {
                y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
                self.mt[kk] = self.mt[kk + M - N] ^ (y >> 1) ^ MAG01[(y & 0x1) as usize];
            }
            y = (self.mt[N - 1] & UPPER_MASK) | (self.mt[0] & LOWER_MASK);
            self.mt[N - 1] = self.mt[M - 1] ^ (y >> 1) ^ MAG01[(y & 0x1) as usize];
            self.index = 0;
        }
        y = self.mt[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9D2C_5680u32;
        y ^= (y << 15) & 0xEFC6_0000u32;
        y ^= y >> 18;
        y
    }

    /// 53-bit IEEE-754 float in `[0.0, 1.0)` (`random_random` in `_randommodule.c`).
    pub fn random(&mut self) -> f64 {
        self.counter += 1;
        self.raw_random()
    }

    fn raw_random(&mut self) -> f64 {
        let a = self.genrand_uint32() >> 5;
        let b = self.genrand_uint32() >> 6;
        ((a as f64) * 67108864.0 + (b as f64)) * (1.0 / 9007199254740992.0)
    }

    /// `getrandbits(k)` for `0 <= k <= 128` (`_random_Random_getrandbits_impl`).
    pub fn getrandbits(&mut self, mut k: u32) -> u128 {
        assert!(k <= 128, "getrandbits supports k <= 128");
        if k == 0 {
            return 0;
        }
        if k <= 32 {
            return (self.genrand_uint32() >> (32 - k)) as u128;
        }
        let words = (k - 1) / 32 + 1;
        let mut acc: u128 = 0;
        for i in 0..words {
            let mut r = self.genrand_uint32();
            if k < 32 {
                r >>= 32 - k;
            }
            acc |= (r as u128) << (i * 32);
            k = k.saturating_sub(32);
        }
        acc
    }

    /// `_randbelow_with_getrandbits(n)` from CPython `Lib/random.py`.
    pub fn randbelow(&mut self, n: u128) -> u128 {
        if n == 0 {
            return 0;
        }
        let k = 128 - n.leading_zeros();
        let mut r = self.getrandbits(k);
        while r >= n {
            r = self.getrandbits(k);
        }
        r
    }

    /// `randint(a, b)` inclusive (`a + _randbelow(b - a + 1)`).
    pub fn randint(&mut self, a: i128, b: i128) -> i128 {
        assert!(a <= b, "empty range for randint");
        self.counter += 1;
        let span = (b - a) as u128 + 1;
        a + (self.randbelow(span) as i128)
    }

    /// `choice(seq)` (`seq[_randbelow(len(seq))]`).
    pub fn choice<'a, T>(&mut self, seq: &'a [T]) -> &'a T {
        assert!(!seq.is_empty(), "Cannot choose from an empty sequence");
        self.counter += 1;
        let idx = self.randbelow(seq.len() as u128) as usize;
        &seq[idx]
    }

    /// `shuffle(seq)` (`Lib/random.py` Fisher-Yates loop).
    /// Note: Python counts `self.counter += len(seq)` in `SeededRNG.shuffle` (`models.py:389`).
    pub fn shuffle<T>(&mut self, seq: &mut [T]) {
        self.counter += seq.len() as u64;
        for i in (1..seq.len()).rev() {
            let j = self.randbelow((i + 1) as u128) as usize;
            seq.swap(i, j);
        }
    }

    /// `sample(population, k)` implementing both the pool branch and the set branch
    /// from CPython `Lib/random.py`.
    /// Note: Python counts `self.counter += k` in `SeededRNG.sample` (`models.py:385`).
    pub fn sample<T: Clone>(&mut self, population: &[T], k: usize) -> Vec<T> {
        let n = population.len();
        assert!(k <= n, "Sample larger than population");
        self.counter += k as u64;
        let mut result = Vec::with_capacity(k);
        let mut setsize: usize = 21;
        if k > 5 {
            let exp = (((k * 3) as f64).ln() / 4.0_f64.ln()).ceil() as u32;
            setsize += 4_usize.pow(exp);
        }
        if n <= setsize {
            let mut pool = population.to_vec();
            for i in 0..k {
                let j = self.randbelow((n - i) as u128) as usize;
                result.push(pool[j].clone());
                pool[j] = pool[n - i - 1].clone();
            }
        } else {
            let mut selected = HashSet::with_capacity(k);
            for _ in 0..k {
                let mut j = self.randbelow(n as u128) as usize;
                while !selected.insert(j) {
                    j = self.randbelow(n as u128) as usize;
                }
                result.push(population[j].clone());
            }
        }
        result
    }

    /// `SeededRNG.weighted_choice(items, weights)` from `models.py:392-401`.
    /// Linear cumulative scan with `<=`.
    pub fn weighted_choice<'a, T>(&mut self, items: &'a [T], weights: &[f64]) -> &'a T {
        assert!(!items.is_empty(), "items must not be empty");
        assert_eq!(
            items.len(),
            weights.len(),
            "items and weights length mismatch"
        );
        self.counter += 1;
        let total: f64 = weights.iter().copied().sum();
        let r = self.raw_random() * total;
        let mut cumulative = 0.0;
        for (item, weight) in items.iter().zip(weights.iter()) {
            cumulative += *weight;
            if r <= cumulative {
                return item;
            }
        }
        items.last().unwrap()
    }

    /// Export state as `[3, [w0..w623, index], gauss_next]`.
    pub fn getstate(&self) -> RngState {
        let mut internal = Vec::with_capacity(N + 1);
        internal.extend_from_slice(&self.mt);
        internal.push(self.index as u32);
        RngState(3, internal, self.gauss_next)
    }

    /// Restore state from `[3, [w0..w623, index], gauss_next]`.
    pub fn setstate(&mut self, state: &RngState) -> Result<(), &'static str> {
        if state.0 != 3 {
            return Err("unsupported RNG state version");
        }
        if state.1.len() != N + 1 {
            return Err("invalid internal state length");
        }
        let idx = state.1[N] as usize;
        if idx > N {
            return Err("invalid MT index");
        }
        self.mt.copy_from_slice(&state.1[..N]);
        self.index = idx;
        self.gauss_next = state.2;
        Ok(())
    }

    /// SHA-256 hex digest of the 624-word state + index (`getstate().1` as little-endian u32s
    /// or canonical JSON `[3, [...], null]`).
    pub fn state_sha256_hex(&self) -> String {
        use sha2::{Digest, Sha256};
        let state_json = serde_json::to_string(&self.getstate()).expect("serialize RngState");
        let digest = Sha256::digest(state_json.as_bytes());
        hex_encode(&digest)
    }
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // preservation: CpythonRng::setstate out-of-bounds / invalid state rejection
    #[test]
    fn test_setstate_out_of_bounds_and_coerce_failures() {
        let mut rng = CpythonRng::from_seed(42);
        let orig = rng.getstate();

        // Version != 3
        let bad_ver = RngState(2, vec![0; N + 1], None);
        assert_eq!(rng.setstate(&bad_ver), Err("unsupported RNG state version"));
        assert_eq!(rng.getstate(), orig);

        // Wrong internal length (< 625 and > 625)
        let short_len = RngState(3, vec![0; N], None);
        assert_eq!(
            rng.setstate(&short_len),
            Err("invalid internal state length")
        );
        let long_len = RngState(3, vec![0; N + 2], None);
        assert_eq!(
            rng.setstate(&long_len),
            Err("invalid internal state length")
        );
        assert_eq!(rng.getstate(), orig);

        // Out-of-bounds MT index (> 624)
        let mut bad_idx_words = vec![0u32; N + 1];
        bad_idx_words[N] = (N as u32) + 1;
        let bad_idx = RngState(3, bad_idx_words.clone(), Some(1.25));
        assert_eq!(rng.setstate(&bad_idx), Err("invalid MT index"));
        assert_eq!(rng.getstate(), orig);

        // coerce_rng_state rejects out-of-bounds MT index, > u32::MAX word, and non-f64 gauss_next
        assert!(coerce_rng_state(&json!([3, bad_idx_words, null])).is_none());
        let mut over_u32 = vec![0u64; N + 1];
        over_u32[0] = (u32::MAX as u64) + 1;
        assert!(coerce_rng_state(&json!([3, over_u32, null])).is_none());
        let valid_words = vec![0u32; N + 1];
        assert!(coerce_rng_state(&json!([3, valid_words, "not_a_float"])).is_none());
    }
}
