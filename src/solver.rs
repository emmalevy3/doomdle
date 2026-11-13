//! AND/OR search over the engine, and the pile-verification entry point.

use std::collections::HashMap;
use std::fmt;

use crate::cards::Card;
use crate::engine::{Action, EngineError, apply, legal_actions};
use crate::state::{Outcome, Player, Puzzle, State};

/// A winning plan. At your decisions it names one action; at the opponent's
/// it answers every action they could take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Strategy {
    /// The game is already won here.
    Win,
    /// Your decision: take `action`, then continue with `next`.
    Mine { action: Action, next: Box<Strategy> },
    /// The opponent's decision: whatever they do, the paired strategy wins.
    Theirs { branches: Vec<(Action, Strategy)> },
}

/// One action of a strategy together with the state it leads to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub actor: Player,
    pub action: Action,
    pub state: State,
}

impl Strategy {
    /// Replays the strategy from `start`, calling `visit` with the depth of
    /// opponent branching and each step. Every branch is visited.
    pub fn walk(
        &self,
        start: &State,
        visit: &mut impl FnMut(usize, &Step),
    ) -> Result<(), EngineError> {
        self.walk_at(start, 0, visit)
    }

    fn walk_at(
        &self,
        state: &State,
        depth: usize,
        visit: &mut impl FnMut(usize, &Step),
    ) -> Result<(), EngineError> {
        match self {
            Strategy::Win => Ok(()),
            Strategy::Mine { action, next } => {
                let step = step(state, action)?;
                visit(depth, &step);
                next.walk_at(&step.state, depth, visit)
            }
            Strategy::Theirs { branches } => {
                let nested = usize::from(branches.len() > 1);
                for (action, next) in branches {
                    let step = step(state, action)?;
                    visit(depth + nested, &step);
                    next.walk_at(&step.state, depth + nested, visit)?;
                }
                Ok(())
            }
        }
    }

    /// The strategy as a single sequence of steps. Fails if the opponent ever
    /// has more than one option, since the line then branches.
    pub fn line(&self, start: &State) -> Result<Vec<Step>, EngineError> {
        let mut steps = Vec::new();
        let mut branched = false;
        self.walk(start, &mut |depth, step| {
            if depth > 0 {
                branched = true;
            }
            steps.push(step.clone());
        })?;
        if branched {
            return Err(EngineError::Internal(
                "the strategy branches on opponent choices",
            ));
        }
        Ok(steps)
    }
}

fn step(state: &State, action: &Action) -> Result<Step, EngineError> {
    let actor = state.actor().ok_or(EngineError::Internal(
        "strategy continues past the end of the game",
    ))?;
    Ok(Step {
        actor,
        action: action.clone(),
        state: apply(state, action)?,
    })
}

impl fmt::Display for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_strategy(self, f, 0, &mut 1)
    }
}

fn fmt_strategy(
    strategy: &Strategy,
    f: &mut fmt::Formatter<'_>,
    indent: usize,
    step: &mut usize,
) -> fmt::Result {
    let pad = "  ".repeat(indent);
    match strategy {
        Strategy::Win => writeln!(f, "{pad}You win."),
        Strategy::Mine { action, next } => {
            writeln!(f, "{pad}{step}. {action}")?;
            *step += 1;
            fmt_strategy(next, f, indent, step)
        }
        Strategy::Theirs { branches } => {
            if let [(action, next)] = branches.as_slice() {
                writeln!(f, "{pad}{step}. Opponent: {action}")?;
                *step += 1;
                return fmt_strategy(next, f, indent, step);
            }
            for (action, next) in branches {
                writeln!(f, "{pad}If the opponent: {action}")?;
                let mut sub = *step;
                fmt_strategy(next, f, indent + 1, &mut sub)?;
            }
            Ok(())
        }
    }
}

enum Memo {
    InProgress,
    Lost,
    Won(Strategy),
}

/// Finds a winning strategy from `start`, if one exists. Your decisions are
/// OR nodes (one winning action suffices); the opponent's are AND nodes (every
/// action must still lose for them).
pub fn solve(start: &State) -> Option<Strategy> {
    let mut memo = HashMap::new();
    search(start, &mut memo)
}

fn search(state: &State, memo: &mut HashMap<State, Memo>) -> Option<Strategy> {
    match state.outcome {
        Some(Outcome::Win) => return Some(Strategy::Win),
        Some(Outcome::Loss(_)) => return None,
        None => {}
    }
    match memo.get(state) {
        Some(Memo::InProgress | Memo::Lost) => return None,
        Some(Memo::Won(strategy)) => return Some(strategy.clone()),
        None => {}
    }
    memo.insert(state.clone(), Memo::InProgress);

    let result = match state.actor() {
        Some(Player::You) => search_mine(state, memo),
        Some(Player::Opponent) => search_theirs(state, memo),
        None => None,
    };

    let entry = match &result {
        Some(strategy) => Memo::Won(strategy.clone()),
        None => Memo::Lost,
    };
    memo.insert(state.clone(), entry);
    result
}

fn search_mine(state: &State, memo: &mut HashMap<State, Memo>) -> Option<Strategy> {
    for action in legal_actions(state) {
        let Ok(next) = apply(state, &action) else {
            continue;
        };
        if let Some(strategy) = search(&next, memo) {
            return Some(Strategy::Mine {
                action,
                next: Box::new(strategy),
            });
        }
    }
    None
}

fn search_theirs(state: &State, memo: &mut HashMap<State, Memo>) -> Option<Strategy> {
    let actions = legal_actions(state);
    if actions.is_empty() {
        return None;
    }
    let mut branches = Vec::with_capacity(actions.len());
    for action in actions {
        let next = apply(state, &action).ok()?;
        let strategy = search(&next, memo)?;
        branches.push((action, strategy));
    }
    Some(Strategy::Theirs { branches })
}

/// The result of checking a submitted pile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The pile wins; here is a line that does it from the post-Doomsday state.
    Wins {
        start: Box<State>,
        strategy: Strategy,
    },
    /// No line wins against every opponent response.
    Fails,
}

/// Checks a pile for a puzzle: builds the post-Doomsday state and searches it.
/// Returns an error only if the pile itself is not a legal Doomsday choice.
pub fn verify(puzzle: Puzzle, pile: &[Card]) -> Result<Verdict, EngineError> {
    let start = State::with_pile(puzzle, pile)?;
    Ok(match solve(&start) {
        Some(strategy) => Verdict::Wins {
            start: Box::new(start),
            strategy,
        },
        None => Verdict::Fails,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Choice;
    use crate::mana::{Color, ManaPool};
    use crate::state::test_support::main_phase;
    use crate::state::{Permanent, PlayerState};

    #[test]
    fn solved_line_replays_to_a_win() {
        let start = main_phase(PlayerState {
            hand: vec![Card::ThassasOracle],
            battlefield: vec![
                Permanent::untapped(Card::Island),
                Permanent::untapped(Card::UndergroundSea),
            ],
            library: vec![Card::Swamp],
            ..PlayerState::default()
        });
        let strategy = solve(&start).expect("a winning line");
        let steps = strategy.line(&start).unwrap();
        let last = steps.last().unwrap();
        assert_eq!(last.state.outcome, Some(Outcome::Win));
        // Tap, tap, cast, pass, pass (Oracle resolves), pass, pass (trigger).
        assert_eq!(steps.len(), 7);
        assert!(steps.iter().any(|s| matches!(
            s.action,
            Action::CastSpell {
                card: Card::ThassasOracle,
                ..
            }
        )));
        assert!(steps.iter().any(|s| s.actor == Player::Opponent));
        assert!(strategy.to_string().ends_with("You win.\n"));
    }

    #[test]
    fn no_line_when_mana_is_short() {
        let start = main_phase(PlayerState {
            hand: vec![Card::ThassasOracle],
            battlefield: vec![Permanent::untapped(Card::Island)],
            ..PlayerState::default()
        });
        assert_eq!(solve(&start), None);
    }

    #[test]
    fn solver_uses_brainstorm_put_back_to_find_a_win() {
        // Brainstorm must put back the two lands and keep Oracle.
        let start = main_phase(PlayerState {
            hand: vec![Card::Brainstorm],
            mana_pool: ManaPool::of(Color::U, 3),
            library: vec![Card::Island, Card::ThassasOracle, Card::Swamp],
            ..PlayerState::default()
        });
        let strategy = solve(&start).expect("a winning line");
        let steps = strategy.line(&start).unwrap();
        assert!(steps.iter().any(|s| matches!(
            s.action,
            Action::Choose(Choice::BrainstormPutBack([a, b]))
                if a != Card::ThassasOracle && b != Card::ThassasOracle
        )));
        assert_eq!(steps.last().unwrap().state.outcome, Some(Outcome::Win));
    }

    #[test]
    fn memo_shares_states_reached_in_different_orders() {
        // Two Islands and no spell: every path loses, but the search must
        // finish quickly and return None rather than exploring forever.
        let start = main_phase(PlayerState {
            battlefield: vec![
                Permanent::untapped(Card::Island),
                Permanent::untapped(Card::Swamp),
                Permanent::untapped(Card::LotusPetal),
            ],
            ..PlayerState::default()
        });
        let mut memo = HashMap::new();
        assert_eq!(search(&start, &mut memo), None);
        // Tapping Island then Swamp or Swamp then Island is one state.
        let both = apply(
            &apply(
                &start,
                &Action::TapForMana {
                    source: Card::Island,
                    color: Color::U,
                },
            )
            .unwrap(),
            &Action::TapForMana {
                source: Card::Swamp,
                color: Color::B,
            },
        )
        .unwrap();
        assert!(matches!(memo.get(&both), Some(Memo::Lost)));
    }
}
