//! Doomdle rules engine and solver.
//!
//! The puzzle starts as Doomsday resolves: the player picks five cards from
//! library + graveyard, orders them, and must win this turn. See `CARDS.md`
//! for how every card behaves in this engine and `CLAUDE.md` for scope.

pub mod cards;
pub mod engine;
pub mod mana;
pub mod solver;
pub mod state;

pub use cards::{Card, CardKind, LandType, ManaAbility, TriggerKind};
pub use engine::{Action, Choice, EngineError, apply, legal_actions};
pub use mana::{Color, ManaCost, ManaPool};
pub use solver::{Step, Strategy, Verdict, solve, verify};
pub use state::{
    ChoiceKind, LossReason, Outcome, PendingChoice, Permanent, Phase, Player, PlayerState, Puzzle,
    StackItem, State,
};
