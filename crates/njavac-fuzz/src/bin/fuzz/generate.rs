mod expression;
mod line_terminators;
mod long_branch;
mod statement;

use crate::model::Prog;

pub(super) fn scheduled(index: u64) -> Option<Prog> {
    long_branch::scheduled(index).or_else(|| line_terminators::scheduled(index))
}

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
    definite_assignment_paths: bool,
    #[allow(dead_code)]
    has_ternary: bool,
    #[allow(dead_code)]
    has_loops: bool,
}

const CAPS: ScopeCaps = ScopeCaps {
    decls_in_branches: false,
    boolean_boundaries: true,
    definite_assignment_paths: true,
    has_ternary: false,
    has_loops: false,
};

pub(super) struct Gen {
    pub(super) rng: Rng,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CaseKind;
    use crate::render::render;

    #[test]
    fn schedules_the_complete_compiler_peculiarity_prefix() {
        assert_eq!(
            scheduled(0).unwrap().kind,
            CaseKind::LongConditionalBoundary
        );
        assert_eq!(scheduled(1).unwrap().kind, CaseKind::LongConditionalFat);
        assert_eq!(scheduled(2).unwrap().kind, CaseKind::LongGotoFat);
        assert_eq!(scheduled(3).unwrap().kind, CaseKind::MixedLineTerminators);
        assert!(scheduled(4).is_none());
    }

    #[test]
    fn scheduled_prefix_preserves_the_later_random_stream() {
        let seed = 0x6A09_E667_F3BC_C909;
        let mut mixed = Gen {
            rng: Rng::new(seed),
        };
        let mut random = Gen {
            rng: Rng::new(seed),
        };

        for index in 0..10 {
            let actual = mixed.gen_prog(index);
            let expected = random.gen_random_prog(index);
            if scheduled(index).is_none() {
                assert_eq!(render(&actual), render(&expected));
            }
        }
    }
}
