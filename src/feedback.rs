//! Feedback for a pile that fails.
//!
//! A failed pile has no single line where it "went wrong": the solver proved
//! that every line fails. Two honest substitutes:
//!
//! - [`opponent_culprits`]: would the pile win if the opponent held nothing,
//!   and which card in their hand is the one that stops it?
//! - [`closest_line`]: the losing line that gets furthest (fewest cards left
//!   in your library, then most life), with the opponent playing to keep you
//!   furthest away, and how it ends.

use std::collections::HashMap;
use std::fmt;

use crate::cards::Card;
use crate::engine::{Action, EngineError, apply, legal_actions};
use crate::solver::{Verdict, verify_within};
use crate::state::{Player, Puzzle, State};

/// What the opponent's hand has to do with a failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Culprits {
    /// The pile wins if the opponent's hand is empty.
    pub wins_without_their_hand: bool,
    /// The cards that, taken out of their hand (every copy), let the pile
    /// win. Empty when it takes their whole hand, or when the hand isn't the
    /// problem.
    pub stopped_by: Vec<Card>,
}

/// Which opponent cards stop `pile`. `None` when the opponent holds nothing
/// or a search ran out of `max_nodes`. Each re-check gets its own budget, so
/// with three cards in hand this is at most four searches.
pub fn opponent_culprits(
    puzzle: &Puzzle,
    pile: &[Card],
    max_nodes: usize,
) -> Result<Option<Culprits>, EngineError> {
    if puzzle.opponent.hand.is_empty() {
        return Ok(None);
    }
    let without = |keep: &dyn Fn(Card) -> bool| -> Puzzle {
        let mut puzzle = puzzle.clone();
        puzzle.opponent.hand.retain(|&c| keep(c));
        puzzle
    };
    let wins = |puzzle: Puzzle| -> Result<Option<bool>, EngineError> {
        Ok(match verify_within(puzzle, pile, max_nodes)? {
            Verdict::Wins { .. } => Some(true),
            Verdict::Fails => Some(false),
            Verdict::GaveUp => None,
        })
    };
    let Some(wins_without_their_hand) = wins(without(&|_| false))? else {
        return Ok(None);
    };
    let mut stopped_by = Vec::new();
    if wins_without_their_hand {
        let mut names = puzzle.opponent.hand.clone();
        names.sort();
        names.dedup();
        for card in names {
            match wins(without(&|c| c != card))? {
                Some(true) => stopped_by.push(card),
                Some(false) => {}
                None => return Ok(None),
            }
        }
    }
    Ok(Some(Culprits {
        wins_without_their_hand,
        stopped_by,
    }))
}

/// The losing line that got closest, and where it ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosestLine {
    /// Each action with who took it.
    pub steps: Vec<(Player, Action)>,
    /// The state the line ends in (its `outcome` says how).
    pub end: State,
}

impl ClosestLine {
    pub fn cards_left(&self) -> usize {
        self.end.you.library.len()
    }
}

impl fmt::Display for ClosestLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, (actor, action)) in self.steps.iter().enumerate() {
            match actor {
                Player::You => writeln!(f, "{}. {action}", i + 1)?,
                Player::Opponent => writeln!(f, "{}. Opponent: {action}", i + 1)?,
            }
        }
        match &self.end.outcome {
            Some(outcome) => write!(f, "Then {outcome}."),
            None => write!(f, "The line stops here."),
        }
    }
}

/// How good an end state is for you: a win beats everything, then fewer
/// cards left in your library, then more life, then (to break ties in favour
/// of lines that show something) more cards drawn and more spells cast.
/// Derived `Ord` compares the fields in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Score {
    won: bool,
    cards_left: std::cmp::Reverse<usize>,
    life: i16,
    drawn: u8,
    cast: u8,
}

impl Score {
    fn of(state: &State) -> Score {
        Score {
            won: state.outcome == Some(crate::state::Outcome::Win),
            cards_left: std::cmp::Reverse(state.you.library.len()),
            life: state.you.life,
            drawn: state.you.cards_drawn_this_turn,
            cast: state.you.spells_cast_this_turn,
        }
    }

    /// What a state we are already exploring is worth (a loop): nothing.
    const WORST: Score = Score {
        won: false,
        cards_left: std::cmp::Reverse(usize::MAX),
        life: i16::MIN,
        drawn: 0,
        cast: 0,
    };
}

enum Entry {
    InProgress,
    Done { score: Score, best: Option<Action> },
}

/// Minimax over the game from `start` (a state after the pile is chosen):
/// you pick the action with the best [`Score`], the opponent the worst.
/// `None` if it visits more than `max_nodes` states.
pub fn closest_line(start: &State, max_nodes: usize) -> Option<ClosestLine> {
    let mut search = Closest {
        memo: HashMap::new(),
        remaining: max_nodes,
    };
    search.score(start)?;
    // Follow the best actions recorded in the memo.
    let mut steps = Vec::new();
    let mut state = start.clone();
    while state.outcome.is_none() {
        let Some(Entry::Done {
            best: Some(action), ..
        }) = search.memo.get(&state)
        else {
            break;
        };
        let actor = state.actor()?;
        let next = apply(&state, action).ok()?;
        steps.push((actor, action.clone()));
        state = next;
    }
    Some(ClosestLine { steps, end: state })
}

struct Closest {
    memo: HashMap<State, Entry>,
    remaining: usize,
}

impl Closest {
    /// The score of `state` with both players playing their best, or `None`
    /// once the budget is gone.
    fn score(&mut self, state: &State) -> Option<Score> {
        if state.outcome.is_some() {
            return Some(Score::of(state));
        }
        match self.memo.get(state) {
            Some(Entry::InProgress) => return Some(Score::WORST),
            Some(Entry::Done { score, .. }) => return Some(*score),
            None => {}
        }
        self.remaining = self.remaining.checked_sub(1)?;
        self.memo.insert(state.clone(), Entry::InProgress);

        let mine = state.actor() == Some(Player::You);
        let mut best: Option<(Score, Action)> = None;
        for action in legal_actions(state) {
            let Ok(next) = apply(state, &action) else {
                continue;
            };
            let score = self.score(&next)?;
            let better = match &best {
                None => true,
                Some((b, _)) if mine => score > *b,
                Some((b, _)) => score < *b,
            };
            if better {
                best = Some((score, action));
            }
        }
        let (score, best) = match best {
            Some((score, action)) => (score, Some(action)),
            None => (Score::of(state), None),
        };
        self.memo.insert(state.clone(), Entry::Done { score, best });
        Some(score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{LossReason, Outcome, Permanent, PlayerState};

    fn islands(n: usize) -> Vec<Permanent> {
        vec![Permanent::untapped(Card::Island); n]
    }

    fn puzzle(hand: Vec<Card>, lands: usize, library: Vec<Card>, opponent: PlayerState) -> Puzzle {
        Puzzle {
            you: PlayerState {
                hand,
                battlefield: islands(lands),
                library,
                ..PlayerState::default()
            },
            opponent,
        }
    }

    fn counterspell() -> PlayerState {
        PlayerState {
            hand: vec![Card::Counterspell, Card::Daze],
            battlefield: islands(2),
            ..PlayerState::default()
        }
    }

    #[test]
    fn counterspell_is_named_as_the_card_that_stops_oracle() {
        let p = puzzle(
            vec![Card::ThassasOracle],
            3,
            vec![Card::Swamp],
            counterspell(),
        );
        let culprits = opponent_culprits(&p, &[Card::Swamp], 100_000)
            .unwrap()
            .expect("they hold cards");
        assert!(culprits.wins_without_their_hand);
        // With a third Island, Daze alone is paid for; Counterspell isn't.
        assert_eq!(culprits.stopped_by, vec![Card::Counterspell]);
    }

    #[test]
    fn no_culprit_when_the_pile_fails_anyway_or_they_hold_nothing() {
        // One Island: Oracle can't be cast at all.
        let p = puzzle(
            vec![Card::ThassasOracle],
            1,
            vec![Card::Swamp],
            counterspell(),
        );
        let culprits = opponent_culprits(&p, &[Card::Swamp], 100_000)
            .unwrap()
            .expect("they hold cards");
        assert!(!culprits.wins_without_their_hand);
        assert!(culprits.stopped_by.is_empty());
        let passive = puzzle(
            vec![Card::ThassasOracle],
            1,
            vec![Card::Swamp],
            PlayerState::default(),
        );
        assert_eq!(
            opponent_culprits(&passive, &[Card::Swamp], 100_000).unwrap(),
            None
        );
    }

    #[test]
    fn it_takes_the_whole_hand_when_both_counters_stop_it() {
        let p = puzzle(
            vec![Card::ThassasOracle],
            2,
            vec![Card::Swamp],
            PlayerState {
                hand: vec![Card::Counterspell, Card::ForceOfWill, Card::Daze],
                battlefield: islands(2),
                ..PlayerState::default()
            },
        );
        let culprits = opponent_culprits(&p, &[Card::Swamp], 100_000)
            .unwrap()
            .expect("they hold cards");
        assert!(culprits.wins_without_their_hand);
        assert!(culprits.stopped_by.is_empty());
    }

    #[test]
    fn closest_line_gets_as_near_as_it_can() {
        // Two Islands: either Oracle (devotion 2 against three cards, so it
        // misses with three left) or Brainstorm (draws all three, puts two
        // back: two left), not both. Two left is closer.
        let p = puzzle(
            vec![Card::ThassasOracle, Card::Brainstorm],
            2,
            vec![Card::Swamp, Card::Plains, Card::Mountain],
            PlayerState::default(),
        );
        let pile = [Card::Swamp, Card::Plains, Card::Mountain];
        let start = State::with_pile(p, &pile).unwrap();
        let line = closest_line(&start, 1_000_000).expect("within budget");
        assert_eq!(line.end.outcome, Some(Outcome::Loss(LossReason::TurnEnded)));
        assert_eq!(line.cards_left(), 2);
        assert_eq!(line.end.you.life, 10);
        let text = line.to_string();
        assert!(
            text.ends_with("Then the turn ends without a win."),
            "{text}"
        );
    }

    #[test]
    fn closest_line_prefers_drawing_deeper() {
        // Probe draws one: two cards left beats three.
        let p = puzzle(
            vec![Card::GitaxianProbe],
            1,
            vec![Card::Swamp, Card::Plains, Card::Mountain],
            PlayerState::default(),
        );
        let pile = [Card::Swamp, Card::Plains, Card::Mountain];
        let start = State::with_pile(p, &pile).unwrap();
        let line = closest_line(&start, 1_000_000).expect("within budget");
        assert_eq!(line.cards_left(), 2);
        assert!(line.steps.iter().any(|(_, a)| matches!(
            a,
            Action::CastSpell {
                card: Card::GitaxianProbe,
                ..
            }
        )));
    }

    #[test]
    fn the_opponent_plays_to_keep_you_far_away() {
        // Probe against Spell Pierce with no mana to pay: they counter it.
        let p = puzzle(
            vec![Card::GitaxianProbe],
            1,
            vec![Card::Swamp, Card::Plains, Card::Mountain],
            PlayerState {
                hand: vec![Card::SpellPierce],
                battlefield: islands(1),
                ..PlayerState::default()
            },
        );
        let pile = [Card::Swamp, Card::Plains, Card::Mountain];
        let start = State::with_pile(p, &pile).unwrap();
        let line = closest_line(&start, 1_000_000).expect("within budget");
        assert_eq!(line.cards_left(), 3);
        assert!(line.steps.iter().any(|(who, a)| *who == Player::Opponent
            && matches!(
                a,
                Action::CastSpell {
                    card: Card::SpellPierce,
                    ..
                }
            )));
        assert!(line.to_string().contains("Opponent: Cast Spell Pierce"));
    }

    #[test]
    fn a_tiny_budget_gives_nothing() {
        let p = puzzle(
            vec![Card::GitaxianProbe],
            1,
            vec![Card::Swamp, Card::Plains],
            PlayerState::default(),
        );
        let start = State::with_pile(p, &[Card::Swamp, Card::Plains]).unwrap();
        assert_eq!(closest_line(&start, 1), None);
    }
}
