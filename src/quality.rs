//! How good a puzzle is as a puzzle: tries every legal pile and measures how
//! much the choice of pile matters (how many piles win, how many distinct
//! answers there are, which candidates are decoys, whether order matters).

use std::collections::BTreeSet;
use std::fmt;
use std::thread;

use serde::{Deserialize, Serialize};

use crate::cards::Card;
use crate::engine::{Action, EngineError, apply, legal_actions};
use crate::solver::{Search, SearchResult};
use crate::state::{Puzzle, State};

/// A worker's memo is cleared past this many states, to bound memory.
const MEMO_CAP: usize = 250_000;

/// How hard a puzzle is, from the share of all piles that win (the chance one
/// random guess wins): Hard under [`HARD_BELOW`], Medium under
/// [`MEDIUM_BELOW`], Easy up to [`EASY_UP_TO`]. Above that it is too easy to
/// generate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

/// Shares below this are Hard.
pub const HARD_BELOW: f64 = 0.005;
/// Shares below this (and at least [`HARD_BELOW`]) are Medium.
pub const MEDIUM_BELOW: f64 = 0.02;
/// Shares up to this (and at least [`MEDIUM_BELOW`]) are Easy. Set from
/// play-testing: 2.86% of piles winning felt easy but fine, 5.36% too easy.
pub const EASY_UP_TO: f64 = 0.03;

impl Difficulty {
    /// The tier for a share of winning piles, or `None` if it is too easy.
    pub fn from_share(share: f64) -> Option<Difficulty> {
        if share < HARD_BELOW {
            Some(Difficulty::Hard)
        } else if share < MEDIUM_BELOW {
            Some(Difficulty::Medium)
        } else if share <= EASY_UP_TO {
            Some(Difficulty::Easy)
        } else {
            None
        }
    }
}

impl fmt::Display for Difficulty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `pad` honours width and alignment, so tables can line tiers up.
        f.pad(match self {
            Difficulty::Easy => "Easy",
            Difficulty::Medium => "Medium",
            Difficulty::Hard => "Hard",
        })
    }
}

/// A difficulty by name, any case ("hard").
impl std::str::FromStr for Difficulty {
    type Err = String;

    fn from_str(s: &str) -> Result<Difficulty, String> {
        match s.to_ascii_lowercase().as_str() {
            "easy" => Ok(Difficulty::Easy),
            "medium" => Ok(Difficulty::Medium),
            "hard" => Ok(Difficulty::Hard),
            _ => Err(format!("unknown difficulty {s:?} (easy, medium or hard)")),
        }
    }
}

/// How to rate a puzzle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateOptions {
    /// The search budget for each pile, in distinct states.
    pub per_pile_budget: usize,
    /// How many threads search piles at once (at least 1).
    pub threads: usize,
    /// Also re-check losing piles against an empty opponent hand, to count the
    /// piles their hand answers. Roughly doubles the work when they hold cards.
    pub opponent_check: bool,
}

impl Default for RateOptions {
    fn default() -> RateOptions {
        RateOptions {
            per_pile_budget: 200_000,
            threads: thread::available_parallelism().map_or(1, |n| n.get()),
            opponent_check: true,
        }
    }
}

/// One winning set of cards (order ignored) and how many of its orderings win.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WinningSet {
    /// The cards, sorted.
    pub cards: Vec<Card>,
    /// The winning orderings, top first.
    pub orderings: Vec<Vec<Card>>,
    /// How many distinct orderings the set has in all.
    pub total_orderings: usize,
}

impl WinningSet {
    /// True if some ordering of these cards loses.
    pub fn order_matters(&self) -> bool {
        self.orderings.len() < self.total_orderings
    }
}

/// What trying every pile found.
#[derive(Clone, Debug, PartialEq)]
pub struct Rating {
    /// Cards in library and graveyard to choose from.
    pub candidates: usize,
    /// Cards in the pile: five, or all candidates if fewer.
    pub pile_size: usize,
    /// Distinct ordered piles tried.
    pub piles_tried: usize,
    /// The winning piles, grouped by card set.
    pub winning_sets: Vec<WinningSet>,
    /// Distinct answers: winning sets with their filler removed. A card is
    /// filler if swapping it for any other available candidate still wins.
    pub ideas: usize,
    /// Candidate names that appear in no winning pile.
    pub decoys: Vec<Card>,
    /// Losing piles that would win if the opponent's hand were empty (only
    /// counted with [`RateOptions::opponent_check`]).
    pub answered_by_opponent: Option<usize>,
    /// Piles whose search ran out of budget; the rating is incomplete if any.
    pub gave_up: usize,
}

impl Rating {
    /// How many ordered piles win.
    pub fn winning_piles(&self) -> usize {
        self.winning_sets.iter().map(|s| s.orderings.len()).sum()
    }

    /// The share of all piles that win: the chance one random guess wins.
    pub fn winning_share(&self) -> f64 {
        if self.piles_tried == 0 {
            return 0.0;
        }
        self.winning_piles() as f64 / self.piles_tried as f64
    }

    /// The difficulty tier, or `None` if no pile wins or too many do.
    pub fn difficulty(&self) -> Option<Difficulty> {
        if self.winning_sets.is_empty() {
            return None;
        }
        Difficulty::from_share(self.winning_share())
    }

    /// True if every winning set has a losing ordering, so order always matters.
    pub fn order_matters(&self) -> bool {
        !self.winning_sets.is_empty() && self.winning_sets.iter().all(WinningSet::order_matters)
    }

    /// The reasons this puzzle falls short of `criteria`; empty if it passes.
    pub fn check(&self, criteria: &Criteria) -> Vec<String> {
        let mut reasons = Vec::new();
        if self.pile_size < criteria.pile_size {
            reasons.push(format!(
                "the pile is {} cards, not {}",
                self.pile_size, criteria.pile_size
            ));
        }
        if self.candidates < criteria.min_candidates {
            reasons.push(format!(
                "only {} candidates (needs at least {})",
                self.candidates, criteria.min_candidates
            ));
        }
        if self.gave_up > 0 {
            reasons.push(format!(
                "{} piles could not be decided within the budget",
                self.gave_up
            ));
        }
        if self.winning_sets.is_empty() {
            reasons.push("no pile wins".to_string());
        }
        if let Some(max) = criteria.max_ideas
            && self.ideas > max
        {
            reasons.push(format!("{} distinct answers (at most {max})", self.ideas));
        }
        if let Some(max) = criteria.max_sets
            && self.winning_sets.len() > max
        {
            reasons.push(format!(
                "{} winning card sets (at most {max})",
                self.winning_sets.len()
            ));
        }
        if let Some(max) = criteria.max_share
            && self.winning_share() > max
        {
            reasons.push(format!(
                "{:.2}% of piles win (at most {:.2}%)",
                self.winning_share() * 100.0,
                max * 100.0
            ));
        }
        if let Some(min) = criteria.min_decoys
            && self.decoys.len() < min
        {
            reasons.push(format!(
                "{} decoys (needs at least {min})",
                self.decoys.len()
            ));
        }
        if criteria.order_must_matter && !self.winning_sets.is_empty() && !self.order_matters() {
            reasons.push("some winning set wins in every order".to_string());
        }
        reasons
    }
}

/// The bar a generated puzzle must clear. The defaults are the rules agreed so
/// far: a 5-card pile from at least 7 candidates, a winning pile, nothing
/// undecided, and no more than [`EASY_UP_TO`] of piles winning (anything
/// easier has no difficulty tier).
#[derive(Clone, Debug, PartialEq)]
pub struct Criteria {
    pub pile_size: usize,
    pub min_candidates: usize,
    pub max_ideas: Option<usize>,
    pub max_sets: Option<usize>,
    /// A share between 0 and 1.
    pub max_share: Option<f64>,
    pub min_decoys: Option<usize>,
    pub order_must_matter: bool,
}

impl Default for Criteria {
    fn default() -> Criteria {
        Criteria {
            pile_size: 5,
            min_candidates: 7,
            max_ideas: None,
            max_sets: None,
            max_share: Some(EASY_UP_TO),
            min_decoys: None,
            order_must_matter: false,
        }
    }
}

/// How one pile's search ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PileResult {
    Won,
    Lost,
    GaveUp,
}

/// Tries every legal pile of `puzzle` and measures the result.
pub fn rate(puzzle: &Puzzle, options: &RateOptions) -> Result<Rating, EngineError> {
    let start = State::new(puzzle.clone());
    let piles: Vec<Vec<Card>> = legal_actions(&start)
        .into_iter()
        .filter_map(|action| match action {
            Action::ChoosePile(pile) => Some(pile),
            _ => None,
        })
        .collect();
    let candidates = start.you.library.len() + start.you.graveyard.len();
    let pile_size = piles.first().map_or(0, Vec::len);

    let results = search_piles(&start, &piles, options)?;

    let mut answered_by_opponent = None;
    if options.opponent_check && !puzzle.opponent.hand.is_empty() {
        let mut passive = puzzle.clone();
        passive.opponent.hand.clear();
        let passive_start = State::new(passive);
        let losing: Vec<Vec<Card>> = piles
            .iter()
            .zip(&results)
            .filter(|(_, r)| **r == PileResult::Lost)
            .map(|(p, _)| p.clone())
            .collect();
        let passive_results = search_piles(&passive_start, &losing, options)?;
        answered_by_opponent = Some(
            passive_results
                .iter()
                .filter(|r| **r == PileResult::Won)
                .count(),
        );
    }

    let winning: Vec<&Vec<Card>> = piles
        .iter()
        .zip(&results)
        .filter(|(_, r)| **r == PileResult::Won)
        .map(|(p, _)| p)
        .collect();
    let winning_sets = group_by_set(&winning);
    let mut pool = start.you.library.clone();
    pool.extend(&start.you.graveyard);
    let ideas = count_ideas(&winning_sets, &pool);

    let names: BTreeSet<Card> = pool.iter().copied().collect();
    let used: BTreeSet<Card> = winning_sets
        .iter()
        .flat_map(|s| s.cards.iter().copied())
        .collect();
    let decoys = names.difference(&used).copied().collect();

    Ok(Rating {
        candidates,
        pile_size,
        piles_tried: piles.len(),
        winning_sets,
        ideas,
        decoys,
        answered_by_opponent,
        gave_up: results.iter().filter(|r| **r == PileResult::GaveUp).count(),
    })
}

/// Searches each pile from `start` (a `ChoosingPile` state), in order, split
/// over `options.threads` workers. Each worker reuses one memo across its
/// piles; neighbouring piles share a prefix, so workers take contiguous runs.
fn search_piles(
    start: &State,
    piles: &[Vec<Card>],
    options: &RateOptions,
) -> Result<Vec<PileResult>, EngineError> {
    if piles.is_empty() {
        return Ok(Vec::new());
    }
    let threads = options.threads.clamp(1, piles.len());
    let budget = options.per_pile_budget;
    if threads == 1 {
        // No thread at all: WebAssembly in the browser can't start one.
        return search_run(start, piles, budget);
    }
    let chunk = piles.len().div_ceil(threads);
    thread::scope(|scope| {
        let workers: Vec<_> = piles
            .chunks(chunk)
            .map(|run| scope.spawn(move || search_run(start, run, budget)))
            .collect();
        let mut results = Vec::with_capacity(piles.len());
        for worker in workers {
            match worker.join() {
                Ok(run) => results.extend(run?),
                Err(_) => {
                    return Err(EngineError::Internal("a rating worker panicked"));
                }
            }
        }
        Ok(results)
    })
}

fn search_run(
    start: &State,
    piles: &[Vec<Card>],
    budget: usize,
) -> Result<Vec<PileResult>, EngineError> {
    let mut search = Search::new(budget);
    let mut out = Vec::with_capacity(piles.len());
    for pile in piles {
        if search.memo_len() > MEMO_CAP {
            search.forget();
        }
        let after = apply(start, &Action::ChoosePile(pile.clone()))?;
        out.push(match search.run_again(&after, budget).0 {
            SearchResult::Won(_) => PileResult::Won,
            SearchResult::Lost => PileResult::Lost,
            SearchResult::GaveUp => PileResult::GaveUp,
        });
    }
    Ok(out)
}

/// Groups winning piles by their cards (order ignored), in first-seen order.
fn group_by_set(winning: &[&Vec<Card>]) -> Vec<WinningSet> {
    let mut sets: Vec<WinningSet> = Vec::new();
    for pile in winning {
        let mut cards = (*pile).clone();
        cards.sort();
        match sets.iter_mut().find(|s| s.cards == cards) {
            Some(set) => set.orderings.push((*pile).clone()),
            None => {
                let total_orderings = distinct_orderings(&cards);
                sets.push(WinningSet {
                    cards,
                    orderings: vec![(*pile).clone()],
                    total_orderings,
                });
            }
        }
    }
    sets
}

/// How many distinct orders a multiset of cards has: n! / (k1! k2! ...).
fn distinct_orderings(sorted: &[Card]) -> usize {
    let factorial = |n: usize| (1..=n).product::<usize>();
    let mut total = factorial(sorted.len());
    let mut i = 0;
    while i < sorted.len() {
        let run = sorted[i..].iter().take_while(|c| **c == sorted[i]).count();
        total /= factorial(run);
        i += run;
    }
    total
}

/// How many distinct answers the winning sets hold. Each set loses its filler
/// cards (see [`Rating::ideas`]); sets left with the same core are one answer.
/// `pool` is every candidate, one entry per copy.
fn count_ideas(sets: &[WinningSet], pool: &[Card]) -> usize {
    let winning: BTreeSet<&Vec<Card>> = sets.iter().map(|s| &s.cards).collect();
    let copies = |cards: &[Card], card: Card| cards.iter().filter(|c| **c == card).count();
    let names: BTreeSet<Card> = pool.iter().copied().collect();
    let mut cores: BTreeSet<Vec<Card>> = BTreeSet::new();
    for set in sets {
        let mut core = set.cards.clone();
        let distinct: BTreeSet<Card> = set.cards.iter().copied().collect();
        for card in distinct {
            // The cards that could take this one's place: a spare copy is left.
            let swaps: Vec<Card> = names
                .iter()
                .copied()
                .filter(|d| *d != card && copies(pool, *d) > copies(&set.cards, *d))
                .collect();
            let filler = !swaps.is_empty()
                && swaps.iter().all(|d| {
                    let mut swapped = set.cards.clone();
                    if let Some(at) = swapped.iter().position(|c| *c == card) {
                        swapped[at] = *d;
                    }
                    swapped.sort();
                    winning.contains(&swapped)
                });
            if filler && let Some(at) = core.iter().position(|c| *c == card) {
                core.remove(at);
            }
        }
        cores.insert(core);
    }
    cores.len()
}

fn list(cards: &[Card]) -> String {
    cards
        .iter()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

impl fmt::Display for Rating {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} candidates, pile of {}: {} piles tried, {} win ({:.2}%)",
            self.candidates,
            self.pile_size,
            self.piles_tried,
            self.winning_piles(),
            self.winning_share() * 100.0
        )?;
        match self.difficulty() {
            Some(tier) => writeln!(f, "Difficulty: {tier}")?,
            None if self.winning_sets.is_empty() => {}
            None => writeln!(f, "Difficulty: too easy to grade")?,
        }
        writeln!(
            f,
            "{} winning card sets, {} distinct answers; order matters: {}",
            self.winning_sets.len(),
            self.ideas,
            if self.order_matters() { "yes" } else { "no" }
        )?;
        for set in &self.winning_sets {
            writeln!(
                f,
                "  {} — {} of {} orders win, e.g. {}",
                list(&set.cards),
                set.orderings.len(),
                set.total_orderings,
                list(&set.orderings[0])
            )?;
        }
        if self.decoys.is_empty() {
            writeln!(f, "Decoys: none")?;
        } else {
            writeln!(f, "Decoys: {}", list(&self.decoys))?;
        }
        if let Some(n) = self.answered_by_opponent {
            writeln!(f, "Piles the opponent's hand answers: {n}")?;
        }
        if self.gave_up > 0 {
            writeln!(f, "Undecided within the budget: {} piles", self.gave_up)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::puzzle_file::PuzzleFile;
    use crate::solver::{Verdict, verify};

    fn sample(name: &str) -> Puzzle {
        let path = format!("{}/web/puzzles/{name}.json", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).expect("sample puzzle");
        PuzzleFile::parse(&text).expect("valid puzzle").puzzle()
    }

    fn one_thread() -> RateOptions {
        RateOptions {
            per_pile_budget: usize::MAX,
            threads: 1,
            opponent_check: true,
        }
    }

    /// Every sample, rated with a shared memo, agrees with a fresh `verify`
    /// of each pile.
    #[test]
    fn shared_memo_agrees_with_fresh_verify_on_every_sample() {
        for name in [
            "led-brainstorm",
            "bauble-threshold",
            "cavern-wizard",
            "skateboard-tap",
        ] {
            let puzzle = sample(name);
            let rating = rate(&puzzle, &one_thread()).expect("rates");
            let start = State::new(puzzle.clone());
            let mut fresh_wins = Vec::new();
            for action in legal_actions(&start) {
                let Action::ChoosePile(pile) = action else {
                    continue;
                };
                if matches!(verify(puzzle.clone(), &pile), Ok(Verdict::Wins { .. })) {
                    fresh_wins.push(pile);
                }
            }
            let mut rated: Vec<Vec<Card>> = rating
                .winning_sets
                .iter()
                .flat_map(|s| s.orderings.clone())
                .collect();
            rated.sort();
            fresh_wins.sort();
            assert_eq!(rated, fresh_wins, "{name}");
            assert_eq!(rating.gave_up, 0, "{name}");
        }
    }

    #[test]
    fn threads_agree_with_one_thread() {
        let puzzle = sample("led-brainstorm");
        let single = rate(&puzzle, &one_thread()).expect("rates");
        let many = rate(
            &puzzle,
            &RateOptions {
                threads: 4,
                ..one_thread()
            },
        )
        .expect("rates");
        assert_eq!(single, many);
    }

    #[test]
    fn led_brainstorm_has_one_answer_and_order_matters() {
        let rating = rate(&sample("led-brainstorm"), &one_thread()).expect("rates");
        assert_eq!(rating.candidates, 5);
        assert_eq!(rating.pile_size, 5);
        assert_eq!(rating.piles_tried, 120);
        assert_eq!(rating.winning_sets.len(), 1);
        assert_eq!(rating.ideas, 1);
        assert!(rating.order_matters());
        assert!(rating.decoys.is_empty());
        assert_eq!(rating.answered_by_opponent, None);
        assert!(rating.winning_share() > 0.0 && rating.winning_share() < 1.0);
    }

    #[test]
    fn bauble_threshold_fails_the_generated_bar() {
        let rating = rate(&sample("bauble-threshold"), &one_thread()).expect("rates");
        let reasons = rating.check(&Criteria::default());
        assert!(reasons.iter().any(|r| r.contains("the pile is 4 cards")));
        assert!(reasons.iter().any(|r| r.contains("only 4 candidates")));
        assert!(!reasons.iter().any(|r| r.contains("no pile wins")));
    }

    #[test]
    fn the_opponent_check_counts_answered_piles() {
        // Counterspell stops the Swamp pile unless Cavern makes Oracle uncounterable;
        // with Cavern swapped for an Island, the only pile wins only against an empty hand.
        let mut puzzle = sample("cavern-wizard");
        for p in &mut puzzle.you.battlefield {
            if p.card == Card::CavernOfSouls {
                *p = crate::state::Permanent::untapped(Card::Island);
            }
        }
        let rating = rate(&puzzle, &one_thread()).expect("rates");
        assert!(rating.winning_sets.is_empty());
        assert_eq!(rating.answered_by_opponent, Some(1));
        assert!(
            rating
                .check(&Criteria::default())
                .contains(&"no pile wins".to_string())
        );
    }

    fn set(cards: &[Card], wins: usize) -> WinningSet {
        let mut sorted = cards.to_vec();
        sorted.sort();
        WinningSet {
            total_orderings: distinct_orderings(&sorted),
            orderings: vec![sorted.clone(); wins],
            cards: sorted,
        }
    }

    #[test]
    fn distinct_orderings_counts_repeats_once() {
        use Card::*;
        assert_eq!(distinct_orderings(&[Brainstorm, Island, Ponder]), 6);
        assert_eq!(distinct_orderings(&[Island, Island, Swamp]), 3);
        assert_eq!(
            distinct_orderings(&[Island, Island, Island, Island, Island]),
            1
        );
    }

    #[test]
    fn a_card_that_any_candidate_can_replace_is_filler() {
        use Card::*;
        // Candidates: the four core cards plus Island, Swamp and Mountain.
        // Any of the three lands completes the pile, so the land is filler
        // and the three sets are one answer.
        let pool = [
            ThassasOracle,
            Brainstorm,
            Ponder,
            DarkRitual,
            Island,
            Swamp,
            Mountain,
        ];
        let core = [ThassasOracle, Brainstorm, Ponder, DarkRitual];
        let with = |land: Card| {
            let mut cards = core.to_vec();
            cards.push(land);
            set(&cards, 1)
        };
        assert_eq!(
            count_ideas(&[with(Island), with(Swamp), with(Mountain)], &pool),
            1
        );
        // If Mountain doesn't work, the land is not filler: two answers.
        assert_eq!(count_ideas(&[with(Island), with(Swamp)], &pool), 2);
        assert_eq!(count_ideas(&[], &pool), 0);
    }

    #[test]
    fn with_no_spare_candidate_nothing_is_filler() {
        use Card::*;
        let cards = [ThassasOracle, Brainstorm, Ponder, DarkRitual, Island];
        assert_eq!(count_ideas(&[set(&cards, 1)], &cards), 1);
    }

    fn rating_with(sets: Vec<WinningSet>) -> Rating {
        let ideas = sets.len();
        Rating {
            candidates: 8,
            pile_size: 5,
            piles_tried: 6720,
            winning_sets: sets,
            ideas,
            decoys: vec![Card::Island],
            answered_by_opponent: None,
            gave_up: 0,
        }
    }

    #[test]
    fn each_optional_limit_names_its_reason() {
        use Card::*;
        let wins_every_order = WinningSet {
            orderings: vec![vec![]; 120],
            ..set(&[ThassasOracle, Brainstorm, Ponder, Island, DarkRitual], 0)
        };
        let other = set(
            &[LaboratoryManiac, Gush, Consider, Preordain, BlackLotus],
            2,
        );
        let rating = rating_with(vec![wins_every_order, other]);
        assert!(rating.check(&Criteria::default()).is_empty());
        let strict = Criteria {
            max_ideas: Some(1),
            max_sets: Some(1),
            max_share: Some(0.01),
            min_decoys: Some(2),
            order_must_matter: true,
            ..Criteria::default()
        };
        let reasons = rating.check(&strict);
        for needle in [
            "2 distinct answers",
            "2 winning card sets",
            "% of piles win",
            "1 decoys",
            "wins in every order",
        ] {
            assert!(
                reasons.iter().any(|r| r.contains(needle)),
                "{needle}: {reasons:?}"
            );
        }
    }

    #[test]
    fn difficulty_tiers_split_at_their_cutoffs() {
        assert_eq!(Difficulty::from_share(0.0036), Some(Difficulty::Hard));
        assert_eq!(Difficulty::from_share(HARD_BELOW), Some(Difficulty::Medium));
        assert_eq!(Difficulty::from_share(0.0199), Some(Difficulty::Medium));
        assert_eq!(Difficulty::from_share(MEDIUM_BELOW), Some(Difficulty::Easy));
        assert_eq!(Difficulty::from_share(EASY_UP_TO), Some(Difficulty::Easy));
        assert_eq!(Difficulty::from_share(0.0286), Some(Difficulty::Easy));
        assert_eq!(Difficulty::from_share(0.0301), None);
        assert_eq!(Difficulty::from_share(0.0536), None);
        assert_eq!("HARD".parse::<Difficulty>(), Ok(Difficulty::Hard));
        assert!("brutal".parse::<Difficulty>().is_err());
    }

    #[test]
    fn the_default_bar_rejects_puzzles_that_are_too_easy() {
        use Card::*;
        let easy = rating_with(vec![set(
            &[ThassasOracle, Brainstorm, Ponder, Island, DarkRitual],
            600,
        )]);
        assert_eq!(easy.difficulty(), None);
        assert!(
            easy.check(&Criteria::default())
                .iter()
                .any(|r| r.contains("% of piles win"))
        );
        let hard = rating_with(vec![set(
            &[ThassasOracle, Brainstorm, Ponder, Island, DarkRitual],
            6,
        )]);
        assert_eq!(hard.difficulty(), Some(Difficulty::Hard));
        assert!(hard.check(&Criteria::default()).is_empty());
        assert_eq!(rating_with(vec![]).difficulty(), None);
    }

    #[test]
    fn gave_up_and_small_piles_are_reasons() {
        let mut rating = rating_with(vec![]);
        rating.gave_up = 3;
        rating.pile_size = 4;
        rating.candidates = 4;
        let reasons = rating.check(&Criteria::default());
        assert_eq!(reasons.len(), 4, "{reasons:?}");
    }
}
