//! Doomdle rules engine and solver.
//!
//! The puzzle starts as Doomsday resolves: the player picks five cards from
//! library + graveyard, orders them, and must win this turn. See `CARDS.md`
//! for how every card behaves in this engine and `CLAUDE.md` for scope.

pub mod api;
pub mod cards;
pub mod engine;
pub mod feedback;
pub mod generate;
pub mod mana;
pub mod puzzle_file;
pub mod quality;
pub mod solver;
pub mod state;
#[cfg(feature = "wasm")]
pub mod wasm;

pub use api::{DEFAULT_BUDGET, card_names_json, solve_json, verify_json};
pub use cards::{
    AbilityKind, ActivatedAbility, Card, CardKind, CreatureType, CyclingCost, LandFilter, LandType,
    LoyaltyAbility, ManaAbility, TriggerKind,
};
pub use engine::{Action, Choice, EngineError, apply, legal_actions};
pub use feedback::{ClosestLine, Culprits, closest_line, opponent_culprits};
pub use generate::{Attempt, GenerateOptions, Rng, attempt, deal, fingerprint};
pub use mana::{AltCost, Color, ManaCost, ManaPool, Payment};
pub use puzzle_file::{FormatError, PermanentEntry, PermanentFile, PlayerFile, PuzzleFile};
pub use quality::{Criteria, Difficulty, RateOptions, Rating, WinningSet, rate};
pub use solver::{
    PileSearch, SearchResult, Step, Strategy, Verdict, solve, solve_puzzle, solve_puzzle_within,
    solve_within, verify, verify_within,
};
pub use state::{
    ChoiceKind, LossReason, Outcome, PendingChoice, Permanent, Phase, Player, PlayerState, Puzzle,
    SpellMode, StackItem, State, Target, Zone,
};
