//! The ruleset switch: which set of game rules a [`crate::engine::StepEngine`] plays by.

/// Which rules the engine plays by.
///
/// - `Legacy` is the frozen behaviour that the Python vectors (Tier 1) and the Rust snapshot
///   harness (Tier 2) check. It is the default, and every existing constructor produces it.
/// - `V2` is what the playable game will run. New game rules are Rust-only and live here.
///
/// A V2 rule must be gated on the engine's ruleset, so that `Legacy` stays byte-for-byte
/// unchanged (same state, same events, same number and order of RNG draws).
///
/// The ruleset is a runtime field of the engine only. It is not part of `RunState`, the save
/// file, `content_hash` or `LOGIC_VERSION`. A caller that resumes a run must choose the same
/// ruleset the run was played under; the save file does not record it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ruleset {
    /// Frozen behaviour checked by the Python vectors.
    #[default]
    Legacy,
    /// The rules the playable game runs.
    V2,
}
