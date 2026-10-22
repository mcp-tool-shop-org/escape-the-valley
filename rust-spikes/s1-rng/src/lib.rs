//! Spike S1: Bit-exact CPython `random.Random` & `SeededRNG` in Rust.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_B0DF;
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7FFF_FFFF;

///Serialized representation matching CPython `random.Random.getstate()` in `.trail/run.json`:
/// `[3, [w0, ..., w623, index], gauss_next]`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RngStateTuple(pub u32, pub Vec<u32>, pub Option<f64>);

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
    pub fn shuffle<T>(&mut self, seq: &mut [T]) {
        self.counter += seq.len() as u64;
        for i in (1..seq.len()).rev() {
            let j = self.randbelow((i + 1) as u128) as usize;
            seq.swap(i, j);
        }
    }

    /// `sample(population, k)` implementing both the pool branch and the set branch
    /// from CPython `Lib/random.py`.
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

    /// `SeededRNG.weighted_choice(items, weights)` from `models.py`.
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
    pub fn getstate(&self) -> RngStateTuple {
        let mut internal = Vec::with_capacity(N + 1);
        internal.extend_from_slice(&self.mt);
        internal.push(self.index as u32);
        RngStateTuple(3, internal, self.gauss_next)
    }

    /// Restore state from `[3, [w0..w623, index], gauss_next]`.
    pub fn setstate(&mut self, state: &RngStateTuple) -> Result<(), &'static str> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_mt::Mt;

    #[derive(Deserialize)]
    struct VectorsFile {
        schema: String,
        seed_vectors: Vec<SeedVector>,
        save_restore_vector: SaveRestoreVector,
    }

    #[derive(Deserialize)]
    struct SeedVector {
        label: String,
        seed: String,
        initial_state: RngStateTuple,
        random_draws: Vec<RandomDraw>,
        getrandbits_draws: Vec<GetRandBitsDraw>,
        randint_draws: Vec<RandIntDraw>,
        choice_draws: Vec<ChoiceDraw>,
        shuffle_draws: Vec<ShuffleDraw>,
        sample_draws: Vec<SampleDraw>,
        weighted_draws: Vec<WeightedDraw>,
        final_state: RngStateTuple,
    }

    #[derive(Deserialize)]
    struct RandomDraw {
        bits: String,
    }

    #[derive(Deserialize)]
    struct GetRandBitsDraw {
        k: u32,
        value: String,
    }

    #[derive(Deserialize)]
    struct RandIntDraw {
        a: String,
        b: String,
        values: Vec<String>,
    }

    #[derive(Deserialize)]
    struct ChoiceDraw {
        len: usize,
        picks: Vec<String>,
    }

    #[derive(Deserialize)]
    struct ShuffleDraw {
        len: usize,
        result: Vec<usize>,
    }

    #[derive(Deserialize)]
    struct SampleDraw {
        name: String,
        n: usize,
        k: usize,
        result: Vec<usize>,
    }

    #[derive(Deserialize)]
    struct WeightedDraw {
        name: String,
        items: Vec<String>,
        weights: Vec<f64>,
        picks: Vec<String>,
    }

    #[derive(Deserialize)]
    struct SaveRestoreVector {
        saved_rng_state: RngStateTuple,
        post_random_bits: Vec<String>,
        post_randint: Vec<String>,
        post_shuffle: Vec<usize>,
        post_sample_pool: Vec<usize>,
        post_sample_set: Vec<usize>,
        post_weighted: Vec<String>,
        post_final_state: RngStateTuple,
    }

    #[test]
    fn rand_mt_crate_matches_handwritten_mt19937() {
        for seed in [0i128, 1, 42, (1 << 32) + 7, -98765432101234567890] {
            let key = CpythonRng::seed_to_key(seed);
            let mut mt_crate = Mt::new_with_key(key.iter().copied());
            let mut our_rng = CpythonRng::from_seed(seed);
            for _ in 0..1000 {
                assert_eq!(mt_crate.next_u32(), our_rng.genrand_uint32());
            }
        }
    }

    #[test]
    fn all_cpython_vectors_match_bit_for_bit() {
        let raw = include_str!("../vectors.json");
        let vf: VectorsFile = serde_json::from_str(raw).expect("valid vectors.json");
        assert_eq!(vf.schema, "etv-s1-rng-v1");

        let mut total_checks = 0usize;

        for sv in &vf.seed_vectors {
            let seed: i128 = sv.seed.parse().expect("valid i128 seed");
            let mut rng = CpythonRng::from_seed(seed);

            // 1. Initial getstate()
            assert_eq!(
                rng.getstate(),
                sv.initial_state,
                "initial_state mismatch for seed {}",
                sv.label
            );
            total_checks += 1;

            // 2. random() 53-bit IEEE-754 bit-exact check
            for (idx, rd) in sv.random_draws.iter().enumerate() {
                let got_bits = rng.random().to_bits();
                let exp_bits: u64 = rd.bits.parse().unwrap();
                assert_eq!(
                    got_bits, exp_bits,
                    "random() bits mismatch on seed {} draw {}",
                    sv.label, idx
                );
                total_checks += 1;
            }

            // 3. getrandbits(k)
            for (idx, gd) in sv.getrandbits_draws.iter().enumerate() {
                let got = rng.getrandbits(gd.k);
                let exp: u128 = gd.value.parse().unwrap();
                assert_eq!(
                    got, exp,
                    "getrandbits({}) mismatch on seed {} draw {}",
                    gd.k, sv.label, idx
                );
                total_checks += 1;
            }

            // 4. randint(a, b)
            for rd in &sv.randint_draws {
                let a: i128 = rd.a.parse().unwrap();
                let b: i128 = rd.b.parse().unwrap();
                for (idx, exp_str) in rd.values.iter().enumerate() {
                    let got = rng.randint(a, b);
                    let exp: i128 = exp_str.parse().unwrap();
                    assert_eq!(
                        got, exp,
                        "randint({}, {}) mismatch on seed {} draw {}",
                        a, b, sv.label, idx
                    );
                    total_checks += 1;
                }
            }

            // 5. choice(seq)
            for cd in &sv.choice_draws {
                let seq: Vec<String> = (0..cd.len).map(|i| format!("item_{i}")).collect();
                for (idx, exp) in cd.picks.iter().enumerate() {
                    let got = rng.choice(&seq);
                    assert_eq!(
                        got, exp,
                        "choice(len={}) mismatch on seed {} draw {}",
                        cd.len, sv.label, idx
                    );
                    total_checks += 1;
                }
            }

            // 6. shuffle(seq)
            for sd in &sv.shuffle_draws {
                let mut seq: Vec<usize> = (0..sd.len).collect();
                rng.shuffle(&mut seq);
                assert_eq!(
                    seq, sd.result,
                    "shuffle(len={}) mismatch on seed {}",
                    sd.len, sv.label
                );
                total_checks += 1;
            }

            // 7. sample(population, k) — both pool and set branches
            for sm in &sv.sample_draws {
                let pop: Vec<usize> = (0..sm.n).collect();
                let got = rng.sample(&pop, sm.k);
                assert_eq!(
                    got, sm.result,
                    "sample({}, n={}, k={}) mismatch on seed {}",
                    sm.name, sm.n, sm.k, sv.label
                );
                total_checks += 1;
            }

            // 8. weighted_choice(items, weights)
            for wd in &sv.weighted_draws {
                for (idx, exp) in wd.picks.iter().enumerate() {
                    let got = rng.weighted_choice(&wd.items, &wd.weights);
                    assert_eq!(
                        got, exp,
                        "weighted_choice({}) mismatch on seed {} draw {}",
                        wd.name, sv.label, idx
                    );
                    total_checks += 1;
                }
            }

            // 9. Final getstate()
            assert_eq!(
                rng.getstate(),
                sv.final_state,
                "final_state mismatch for seed {}",
                sv.label
            );
            total_checks += 1;
        }

        // 10. Save/restore vector from .trail/run.json-shaped tuple
        let srv = &vf.save_restore_vector;
        let mut restored = CpythonRng::from_seed(0);
        restored
            .setstate(&srv.saved_rng_state)
            .expect("valid saved_rng_state");
        assert_eq!(restored.getstate(), srv.saved_rng_state);
        total_checks += 1;

        for exp_bits_str in &srv.post_random_bits {
            let exp_bits: u64 = exp_bits_str.parse().unwrap();
            assert_eq!(restored.random().to_bits(), exp_bits);
            total_checks += 1;
        }
        for exp_val_str in &srv.post_randint {
            let exp_val: i128 = exp_val_str.parse().unwrap();
            assert_eq!(restored.randint(-50, 500), exp_val);
            total_checks += 1;
        }
        let mut shuf: Vec<usize> = (0..10).collect();
        restored.shuffle(&mut shuf);
        assert_eq!(shuf, srv.post_shuffle);
        total_checks += 1;

        let pop15: Vec<usize> = (0..15).collect();
        assert_eq!(restored.sample(&pop15, 4), srv.post_sample_pool);
        total_checks += 1;

        let pop100: Vec<usize> = (0..100).collect();
        assert_eq!(restored.sample(&pop100, 5), srv.post_sample_set);
        total_checks += 1;

        let items = vec![
            "food".to_string(),
            "water".to_string(),
            "ammo".to_string(),
            "parts".to_string(),
        ];
        let weights = vec![2.5, 3.0, 1.0, 0.5];
        for exp_pick in &srv.post_weighted {
            assert_eq!(restored.weighted_choice(&items, &weights), exp_pick);
            total_checks += 1;
        }

        assert_eq!(restored.getstate(), srv.post_final_state);
        total_checks += 1;

        println!("Verified {total_checks} bit-exact CPython RNG vector assertions across 6 seeds + save/restore.");
        assert!(total_checks >= 2500);
    }
}
