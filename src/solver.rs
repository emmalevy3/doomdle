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

    /// The state at the end of every branch of the strategy.
    pub fn outcomes(&self, start: &State) -> Result<Vec<State>, EngineError> {
        let mut leaves = Vec::new();
        self.collect_outcomes(start, &mut leaves)?;
        Ok(leaves)
    }

    fn collect_outcomes(&self, state: &State, leaves: &mut Vec<State>) -> Result<(), EngineError> {
        match self {
            Strategy::Win => leaves.push(state.clone()),
            Strategy::Mine { action, next } => {
                next.collect_outcomes(&step(state, action)?.state, leaves)?;
            }
            Strategy::Theirs { branches } => {
                for (action, next) in branches {
                    next.collect_outcomes(&step(state, action)?.state, leaves)?;
                }
            }
        }
        Ok(())
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

/// How a budgeted search ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchResult {
    Won(Strategy),
    /// Every line was explored and none wins.
    Lost,
    /// The node budget ran out before the search finished.
    GaveUp,
}

/// Finds a winning strategy from `start`, if one exists. Your decisions are
/// OR nodes (one winning action suffices); the opponent's are AND nodes (every
/// action must still lose for them). Unbounded: use [`solve_within`] where
/// a runaway search would be a problem.
pub fn solve(start: &State) -> Option<Strategy> {
    match solve_within(start, usize::MAX) {
        SearchResult::Won(strategy) => Some(strategy),
        SearchResult::Lost | SearchResult::GaveUp => None,
    }
}

/// Like [`solve`], visiting at most `max_nodes` distinct states.
pub fn solve_within(start: &State, max_nodes: usize) -> SearchResult {
    Search::new(max_nodes).run(start).0
}

/// One search with its memo and remaining budget.
struct Search {
    memo: HashMap<State, Memo>,
    remaining: usize,
    gave_up: bool,
}

impl Search {
    fn new(max_nodes: usize) -> Search {
        Search {
            memo: HashMap::new(),
            remaining: max_nodes,
            gave_up: false,
        }
    }

    /// The result and the number of states visited.
    fn run(mut self, start: &State) -> (SearchResult, usize) {
        let budget = self.remaining;
        let result = match self.search(start) {
            Some(strategy) => SearchResult::Won(strategy),
            None if self.gave_up => SearchResult::GaveUp,
            None => SearchResult::Lost,
        };
        (result, budget - self.remaining)
    }

    fn search(&mut self, state: &State) -> Option<Strategy> {
        match state.outcome {
            Some(Outcome::Win) => return Some(Strategy::Win),
            Some(Outcome::Loss(_)) => return None,
            None => {}
        }
        match self.memo.get(state) {
            Some(Memo::InProgress | Memo::Lost) => return None,
            Some(Memo::Won(strategy)) => return Some(strategy.clone()),
            None => {}
        }
        // A new state costs one node. Past the budget everything "loses",
        // and the flag tells the caller not to trust that.
        if self.remaining == 0 {
            self.gave_up = true;
            return None;
        }
        self.remaining -= 1;
        self.memo.insert(state.clone(), Memo::InProgress);

        let result = match state.actor() {
            Some(Player::You) => self.search_mine(state),
            Some(Player::Opponent) => self.search_theirs(state),
            None => None,
        };

        let entry = match &result {
            Some(strategy) => Memo::Won(strategy.clone()),
            None => Memo::Lost,
        };
        self.memo.insert(state.clone(), entry);
        result
    }

    fn search_mine(&mut self, state: &State) -> Option<Strategy> {
        for action in legal_actions(state) {
            if self.gave_up {
                return None;
            }
            let Ok(next) = apply(state, &action) else {
                continue;
            };
            if let Some(strategy) = self.search(&next) {
                return Some(Strategy::Mine {
                    action,
                    next: Box::new(strategy),
                });
            }
        }
        None
    }

    fn search_theirs(&mut self, state: &State) -> Option<Strategy> {
        let actions = legal_actions(state);
        if actions.is_empty() {
            return None;
        }
        let mut branches = Vec::with_capacity(actions.len());
        for action in actions {
            let next = apply(state, &action).ok()?;
            let strategy = self.search(&next)?;
            branches.push((action, strategy));
        }
        Some(Strategy::Theirs { branches })
    }
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
    /// The search ran out of budget (only from [`verify_within`]).
    GaveUp,
}

/// The result of searching every pile of a puzzle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PileSearch {
    /// This pile (top first) wins with this strategy.
    Won { pile: Vec<Card>, strategy: Strategy },
    /// No pile wins.
    Lost,
    /// The budget ran out (only from [`solve_puzzle_within`]).
    GaveUp,
}

/// Searches every legal pile for one that wins: the pile (top first) and its
/// strategy, or `None` if no pile does. Unbounded.
pub fn solve_puzzle(puzzle: Puzzle) -> Result<Option<(Vec<Card>, Strategy)>, EngineError> {
    Ok(match solve_puzzle_within(puzzle, usize::MAX)? {
        PileSearch::Won { pile, strategy } => Some((pile, strategy)),
        PileSearch::Lost | PileSearch::GaveUp => None,
    })
}

/// Like [`solve_puzzle`], with `max_nodes` shared across all the piles tried.
pub fn solve_puzzle_within(puzzle: Puzzle, max_nodes: usize) -> Result<PileSearch, EngineError> {
    let start = State::new(puzzle);
    let mut remaining = max_nodes;
    for action in legal_actions(&start) {
        let Action::ChoosePile(pile) = &action else {
            continue;
        };
        let (result, used) = Search::new(remaining).run(&apply(&start, &action)?);
        remaining -= used;
        match result {
            SearchResult::Won(strategy) => {
                return Ok(PileSearch::Won {
                    pile: pile.clone(),
                    strategy,
                });
            }
            SearchResult::GaveUp => return Ok(PileSearch::GaveUp),
            SearchResult::Lost => {}
        }
    }
    Ok(PileSearch::Lost)
}

/// Checks a pile for a puzzle: builds the post-Doomsday state and searches it.
/// Returns an error only if the pile itself is not a legal Doomsday choice.
/// Unbounded, so it never answers `Verdict::GaveUp`.
pub fn verify(puzzle: Puzzle, pile: &[Card]) -> Result<Verdict, EngineError> {
    verify_within(puzzle, pile, usize::MAX)
}

/// Like [`verify`], visiting at most `max_nodes` states.
pub fn verify_within(
    puzzle: Puzzle,
    pile: &[Card],
    max_nodes: usize,
) -> Result<Verdict, EngineError> {
    let start = State::with_pile(puzzle, pile)?;
    Ok(match solve_within(&start, max_nodes) {
        SearchResult::Won(strategy) => Verdict::Wins {
            start: Box::new(start),
            strategy,
        },
        SearchResult::Lost => Verdict::Fails,
        SearchResult::GaveUp => Verdict::GaveUp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Choice;
    use crate::mana::{Color, ManaPool};
    use crate::state::test_support::{main_phase, versus};
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
    fn a_tiny_budget_gives_up_and_a_large_one_agrees_with_solve() {
        let start = main_phase(PlayerState {
            hand: vec![Card::ThassasOracle],
            battlefield: vec![
                Permanent::untapped(Card::Island),
                Permanent::untapped(Card::UndergroundSea),
            ],
            library: vec![Card::Swamp],
            ..PlayerState::default()
        });
        assert_eq!(solve_within(&start, 1), SearchResult::GaveUp);
        let full = solve(&start).expect("a winning line");
        assert_eq!(solve_within(&start, 10_000), SearchResult::Won(full));
        // A position that loses is Lost, not GaveUp, when the budget suffices.
        let short = main_phase(PlayerState {
            hand: vec![Card::ThassasOracle],
            battlefield: vec![Permanent::untapped(Card::Island)],
            library: vec![Card::Swamp],
            ..PlayerState::default()
        });
        assert_eq!(solve_within(&short, 10_000), SearchResult::Lost);
        assert_eq!(solve_within(&short, 1), SearchResult::GaveUp);
    }

    #[test]
    fn budgeted_pile_search_shares_one_budget() {
        let puzzle = Puzzle {
            you: PlayerState {
                hand: vec![Card::ThassasOracle],
                battlefield: vec![
                    Permanent::untapped(Card::Island),
                    Permanent::untapped(Card::Island),
                ],
                library: vec![Card::Swamp],
                ..PlayerState::default()
            },
            opponent: PlayerState::default(),
        };
        assert_eq!(
            solve_puzzle_within(puzzle.clone(), 1).unwrap(),
            PileSearch::GaveUp
        );
        assert!(matches!(
            solve_puzzle_within(puzzle.clone(), 10_000).unwrap(),
            PileSearch::Won { pile, .. } if pile == vec![Card::Swamp]
        ));
        assert_eq!(
            verify_within(puzzle.clone(), &[Card::Swamp], 1).unwrap(),
            Verdict::GaveUp
        );
        assert!(matches!(
            verify(puzzle, &[Card::Swamp]).unwrap(),
            Verdict::Wins { .. }
        ));
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
        let mut search = Search::new(usize::MAX);
        assert_eq!(search.search(&start), None);
        assert!(!search.gave_up);
        let memo = search.memo;
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

    #[test]
    fn a_responding_opponent_branches_the_strategy() {
        // Oracle on a one-card pile; the opponent holds Counterspell with two
        // Islands, you hold Pact of Negation for it.
        let start = versus(
            PlayerState {
                hand: vec![Card::ThassasOracle, Card::PactOfNegation],
                mana_pool: ManaPool::of(Color::U, 2),
                library: vec![Card::Swamp],
                ..PlayerState::default()
            },
            PlayerState {
                hand: vec![Card::Counterspell],
                battlefield: vec![
                    Permanent::untapped(Card::Island),
                    Permanent::untapped(Card::Island),
                ],
                ..PlayerState::default()
            },
        );
        let strategy = solve(&start).expect("Pact answers Counterspell");
        // The opponent's first decision has two branches, so no single line.
        assert!(strategy.line(&start).is_err());
        let leaves = strategy.outcomes(&start).unwrap();
        assert!(leaves.len() >= 2);
        assert!(leaves.iter().all(|s| s.outcome == Some(Outcome::Win)));
        let text = strategy.to_string();
        assert!(text.contains("If the opponent: Cast Counterspell"));
        assert!(text.contains("Cast Pact of Negation"));
        // Without the Pact the counter wins.
        let mut unprotected = start.clone();
        unprotected.you.hand = vec![Card::ThassasOracle];
        assert_eq!(solve(&unprotected), None);
    }
}
