//! Doomdle rules engine and solver.
//!
//! The puzzle starts as Doomsday resolves: the player picks five cards from
//! library + graveyard, orders them, and must win this turn. See `CARDS.md`
//! for how every card behaves in this engine and `CLAUDE.md` for scope.

pub mod cards;
pub mod engine;
pub mod mana;
pub mod puzzle_file;
pub mod solver;
pub mod state;

pub use cards::{
    AbilityKind, ActivatedAbility, Card, CardKind, CreatureType, CyclingCost, LandFilter, LandType,
    LoyaltyAbility, ManaAbility, TriggerKind,
};
pub use engine::{Action, Choice, EngineError, apply, legal_actions};
pub use mana::{AltCost, Color, ManaCost, ManaPool, Payment};
pub use puzzle_file::{FormatError, PermanentEntry, PermanentFile, PlayerFile, PuzzleFile};
pub use solver::{Step, Strategy, Verdict, solve, solve_puzzle, verify};
pub use state::{
    ChoiceKind, LossReason, Outcome, PendingChoice, Permanent, Phase, Player, PlayerState, Puzzle,
    SpellMode, StackItem, State, Target, Zone,
};
