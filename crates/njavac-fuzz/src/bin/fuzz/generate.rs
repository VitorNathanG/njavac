mod expression;
mod statement;

// SplitMix64 is deterministic, seeded, dependency-free, and rung-invariant.
pub(super) struct Rng {
    state: u64,
}

impl Rng {
    pub(super) fn new(seed: u64) -> Self {
        Rng { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n` (n > 0).
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    fn boolean(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    /// True with probability `num/den`.
    fn ratio(&mut self, num: u32, den: u32) -> bool {
        (self.below(den as usize) as u32) < num
    }

    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

/// The materialization mode a boolean expression is generated in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BoolMode {
    Branch,
    Value,
}

/// Every boundary decision in the generator reads this, so the supported surface
/// is one reviewable structure rather than scattered `if`s.
struct ScopeCaps {
    decls_in_branches: bool,
    boolean_boundaries: bool,
    #[allow(dead_code)]
    has_ternary: bool,
    #[allow(dead_code)]
    has_loops: bool,
}

const CAPS: ScopeCaps = ScopeCaps {
    decls_in_branches: false,
    boolean_boundaries: true,
    has_ternary: false,
    has_loops: false,
};

pub(super) struct Gen {
    pub(super) rng: Rng,
}
