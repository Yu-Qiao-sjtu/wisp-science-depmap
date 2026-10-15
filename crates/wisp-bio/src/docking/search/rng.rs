//! Deterministic RNG for the docking search.
//!
//! The search must be reproducible: the same seed produces the same pose
//! sequence on every platform, every run (#274). Instead of pulling in a
//! general-purpose RNG crate, this module implements the two small,
//! well-tested generators the search needs:
//!
//! * [`SplitMix64`] expands the user seed into independent per-stream
//!   seeds. One MC run consumes exactly one stream, so run `k` of
//!   exhaustiveness `N` uses `stream(seed, k)` — the documented layout
//!   that keeps multi-run results stable regardless of thread scheduling.
//! * [`Rng`] is xoshiro256++ (Blackman & Vigna), the stream itself.
//!   64-bit state, no allocation, `u64`/uniform `f64`/Gaussian steps.
//!
//! Both algorithms are integer/floating-point exact per the reference
//! definitions, so outputs are bit-identical across platforms that share
//! IEEE 754 semantics.

/// SplitMix64: the seed-expansion function (Blackman & Vigna, 2019).
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> SplitMix64 {
        SplitMix64 { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// xoshiro256++ stream. Created only through [`Rng::from_stream`] so runs
/// never share state.
#[derive(Debug, Clone)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    /// Stream `index` of `seed`: the documented per-run layout. Stream 0,
    /// 1, 2, ... are statistically independent because the 256-bit states
    /// are drawn from SplitMix64 expansions of `seed.wrapping_add(index`
    /// mixing), and states never mix across streams.
    pub fn from_stream(seed: u64, index: u64) -> Rng {
        let mut mixer = SplitMix64::new(seed ^ index.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        Rng {
            s: [
                mixer.next_u64(),
                mixer.next_u64(),
                mixer.next_u64(),
                mixer.next_u64(),
            ],
        }
    }

    /// Raw 64 bits.
    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[0]
            .wrapping_add(self.s[3])
            .rotate_left(23)
            .wrapping_add(self.s[0]);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// Uniform in `[0, 1)`.
    pub fn next_uniform(&mut self) -> f64 {
        // 53 significant bits: every representable double in [0, 1) can be
        // drawn and the rounding is uniform to the last bit.
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    /// Uniform in `[-scale, scale)`.
    pub fn next_signed(&mut self, scale: f64) -> f64 {
        (self.next_uniform() * 2.0 - 1.0) * scale
    }

    /// Standard normal deviate (Box-Muller). Draws two uniforms; the second
    /// deviate is discarded, keeping the stream simple and position-free.
    pub fn next_normal(&mut self) -> f64 {
        let u1 = self.next_uniform().max(f64::MIN_POSITIVE);
        let u2 = self.next_uniform();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream_bit_exact() {
        let mut a = Rng::from_stream(42, 0);
        let mut b = Rng::from_stream(42, 0);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seed_or_stream_diverge() {
        let mut base = Rng::from_stream(42, 0);
        let mut other_seed = Rng::from_stream(43, 0);
        let mut other_stream = Rng::from_stream(42, 1);
        let base_bits: Vec<u64> = (0..16).map(|_| base.next_u64()).collect();
        let seed_bits: Vec<u64> = (0..16).map(|_| other_seed.next_u64()).collect();
        let stream_bits: Vec<u64> = (0..16).map(|_| other_stream.next_u64()).collect();
        assert_ne!(base_bits, seed_bits);
        assert_ne!(base_bits, stream_bits);
    }

    #[test]
    fn uniforms_cover_the_unit_interval() {
        let mut rng = Rng::from_stream(7, 0);
        let mut min = 1.0_f64;
        let mut max = 0.0_f64;
        for _ in 0..10_000 {
            let value = rng.next_uniform();
            assert!((0.0..1.0).contains(&value));
            min = min.min(value);
            max = max.max(value);
        }
        assert!(min < 0.001, "expected draws near 0, got {min}");
        assert!(max > 0.999, "expected draws near 1, got {max}");
    }

    #[test]
    fn normals_center_on_zero_with_unit_spread() {
        let mut rng = Rng::from_stream(7, 0);
        let draws: Vec<f64> = (0..20_000).map(|_| rng.next_normal()).collect();
        let mean = draws.iter().sum::<f64>() / draws.len() as f64;
        let variance =
            draws.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / draws.len() as f64;
        assert!(mean.abs() < 0.05, "mean drifted: {mean}");
        assert!((variance - 1.0).abs() < 0.1, "variance off: {variance}");
    }
}
