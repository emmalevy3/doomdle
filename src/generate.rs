//! Random puzzles: deals a game state from the card pool, then keeps it only
//! if the rater ([`crate::quality`]) finds that the choice of pile matters.
//! Deterministic for a given seed, and free of clocks and the OS, so it can
//! also run in WebAssembly.

use crate::cards::{Card, CreatureType};
use crate::engine::EngineError;
use crate::mana::ManaPool;
use crate::puzzle_file::{PermanentEntry, PermanentFile, PlayerFile, PuzzleFile};
use crate::quality::{Criteria, Difficulty, RateOptions, Rating, rate};

/// SplitMix64: a tiny, fast, seedable random number generator. Not for
/// cryptography; plenty for dealing cards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n` (`n` must be at least 1).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }

    /// A number in `low..=high`.
    pub fn between(&mut self, low: usize, high: usize) -> usize {
        low + self.below(high - low + 1)
    }

    /// True `percent` times in a hundred.
    pub fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    /// One item of a non-empty list.
    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

// The pools the dealer draws from, grouped like CARDS.md's sections.

const WIN_CONDITIONS: [Card; 4] = [
    Card::ThassasOracle,
    Card::LaboratoryManiac,
    Card::JaceWielderOfMysteries,
    Card::DoctorDoom,
];

const DRAW: [Card; 24] = [
    Card::Brainstorm,
    Card::Ponder,
    Card::Preordain,
    Card::Consider,
    Card::AncestralRecall,
    Card::GitaxianProbe,
    Card::StreetWraith,
    Card::NightsWhisper,
    Card::Meditate,
    Card::IdeasUnbound,
    Card::Manamorphose,
    Card::Gush,
    Card::Timetwister,
    Card::RecklessImpulse,
    Card::FlowState,
    Card::FranticSearch,
    Card::QuantumRiddler,
    Card::TamiyoInquisitiveStudent,
    Card::ThundertrapTrainer,
    Card::AetherSpellbomb,
    Card::EdgeOfAutumn,
    Card::MishrasBauble,
    Card::ChromaticStar,
    Card::WizardsRockets,
];

const MANA: [Card; 9] = [
    Card::DarkRitual,
    Card::CabalRitual,
    Card::LionsEyeDiamond,
    Card::BlackLotus,
    Card::LotusPetal,
    Card::MoxSapphire,
    Card::MoxJet,
    Card::SolRing,
    Card::Gleemox,
];

const PILE_LANDS: [Card; 7] = [
    Card::Island,
    Card::Swamp,
    Card::UndergroundSea,
    Card::UndercitySewers,
    Card::MeticulousArchive,
    Card::PollutedDelta,
    Card::FloodedStrand,
];

/// Extra cards that may start in your hand besides a win condition.
const HAND: [Card; 14] = [
    Card::Brainstorm,
    Card::Ponder,
    Card::LionsEyeDiamond,
    Card::DarkRitual,
    Card::LotusPetal,
    Card::Unearth,
    Card::GitaxianProbe,
    Card::StreetWraith,
    Card::ForceOfWill,
    Card::Silence,
    Card::VeilOfSummer,
    Card::Duress,
    Card::Thoughtseize,
    Card::Pyroblast,
];

const YOUR_LANDS: [Card; 6] = [
    Card::Island,
    Card::Swamp,
    Card::UndergroundSea,
    Card::VolcanicIsland,
    Card::Tundra,
    Card::Badlands,
];

const YOUR_ROCKS: [Card; 3] = [Card::MoxSapphire, Card::MoxJet, Card::SolRing];

const HASTE: [Card; 2] = [Card::LightningGreaves, Card::Skateboard];

/// What the opponent may hold: the instants that answer your spells.
const INTERACTION: [Card; 18] = [
    Card::Counterspell,
    Card::ForceOfWill,
    Card::ForceOfNegation,
    Card::Daze,
    Card::SpellPierce,
    Card::Miscalculation,
    Card::MentalMisstep,
    Card::SpellSnare,
    Card::Stifle,
    Card::PactOfNegation,
    Card::Flusterstorm,
    Card::ConsignToMemory,
    Card::MindbreakTrap,
    Card::Pyroblast,
    Card::Hydroblast,
    Card::SwordsToPlowshares,
    Card::SnuffOut,
    Card::SurgicalExtraction,
];

const THEIR_LANDS: [Card; 5] = [
    Card::Island,
    Card::Tundra,
    Card::VolcanicIsland,
    Card::UndergroundSea,
    Card::Plateau,
];

const HATE: [Card; 6] = [
    Card::Narset,
    Card::OrcishBowmasters,
    Card::LeylineOfTheVoid,
    Card::GrafdiggersCage,
    Card::Thalia,
    Card::ArchonOfEmeria,
];

/// The life a card in your pool costs or loses with no way around it, from
/// CARDS.md. (Gitaxian Probe is absent: it can always be paid with {U}.)
fn life_cost(card: Card) -> i16 {
    match card {
        Card::StreetWraith | Card::NightsWhisper | Card::Thoughtseize => 2,
        Card::PollutedDelta | Card::FloodedStrand | Card::ForceOfWill => 1,
        _ => 0,
    }
}

/// True if `card` could be paid for at `life` (after Doomsday): paying its
/// life would leave you above 0.
fn affordable(card: Card, life: i16) -> bool {
    life_cost(card) < life
}

/// `pool` without the cards whose life cost can't be paid at `life`.
fn payable(pool: &[Card], life: i16) -> Vec<Card> {
    pool.iter()
        .copied()
        .filter(|c| affordable(*c, life))
        .collect()
}

/// Life after Doomsday's loss of half, rounded up.
fn after_doomsday(life: i16) -> i16 {
    life - (life + 1) / 2
}

/// At most this many copies of a card among the candidates.
const MAX_COPIES: usize = 2;

fn tapped(card: Card) -> PermanentFile {
    PermanentFile::Full(PermanentEntry {
        card,
        tapped: true,
        summoning_sick: false,
        loyalty: None,
        creature_type: None,
        greaves: false,
        skateboard: false,
    })
}

fn cavern(creature_type: CreatureType) -> PermanentFile {
    PermanentFile::Full(PermanentEntry {
        card: Card::CavernOfSouls,
        tapped: false,
        summoning_sick: false,
        loyalty: None,
        creature_type: Some(creature_type),
        greaves: false,
        skateboard: false,
    })
}

/// Adds a random card from `pool` to `zone`, keeping each card to
/// [`MAX_COPIES`] across `zone` and `other`.
fn add_from(rng: &mut Rng, pool: &[Card], zone: &mut Vec<Card>, other: &[Card]) {
    for _ in 0..20 {
        let card = rng.pick(pool);
        let copies = zone.iter().chain(other).filter(|c| **c == card).count();
        if copies < MAX_COPIES {
            zone.push(card);
            return;
        }
    }
}

/// Deals one random game state. It is legal, keeps the opponent within
/// CARDS.md's limits (at most 3 cards in hand and 3 mana sources) and offers
/// 7 to 9 candidates in all, 0 to 2 of them in the graveyard (more would
/// multiply the piles to rate: 9 candidates are 15,120 piles, 11 are 55,440). Whether it is a
/// good puzzle is for the rater to say.
pub fn deal(rng: &mut Rng) -> PuzzleFile {
    // Life comes first, so no card is dealt whose life cost can't be paid.
    let life = 2 * rng.between(2, 10) as i16;
    let left = after_doomsday(life);
    let draw = payable(&DRAW, left);
    let hand_pool = payable(&HAND, left);
    let pile_lands = payable(&PILE_LANDS, left);

    let win = rng.pick(&WIN_CONDITIONS);
    let mut hand = Vec::new();
    let mut library = Vec::new();
    let mut graveyard = Vec::new();
    let mut battlefield = Vec::new();

    if rng.chance(40) {
        hand.push(win);
    } else {
        library.push(win);
    }
    for _ in 0..rng.between(0, 2) {
        add_from(rng, &hand_pool, &mut hand, &[]);
    }

    // Three to five mana sources, mostly lands; a few already tapped for
    // mana that was spent before Doomsday.
    let sources = rng.between(3, 5);
    for _ in 0..sources {
        let card = if rng.chance(20) {
            rng.pick(&YOUR_ROCKS)
        } else {
            rng.pick(&YOUR_LANDS)
        };
        battlefield.push(if rng.chance(20) {
            tapped(card)
        } else {
            PermanentFile::Name(card)
        });
    }
    if rng.chance(10) {
        battlefield.push(cavern(CreatureType::Wizard));
    }
    if win == Card::DoctorDoom {
        // Doom needs haste to tap; offer it on the table or in the library.
        let haste = rng.pick(&HASTE);
        if rng.chance(50) {
            battlefield.push(PermanentFile::Name(haste));
        } else {
            library.push(haste);
        }
    }

    let in_graveyard = rng.between(0, 2);
    let library_size = rng.between(7, 9) - in_graveyard;
    while library.len() < library_size {
        let roll = rng.below(100);
        let pool: &[Card] = if roll < 50 {
            &draw
        } else if roll < 85 {
            &MANA
        } else {
            &pile_lands
        };
        let before = library.len();
        add_from(rng, pool, &mut library, &graveyard);
        if library.len() == before {
            break;
        }
    }
    for _ in 0..in_graveyard {
        let pool: &[Card] = if rng.chance(70) { &draw } else { &MANA };
        add_from(rng, pool, &mut graveyard, &library);
    }

    let you = PlayerFile {
        life,
        hand,
        battlefield,
        library,
        graveyard,
        // No floating mana: players found it too easy to overlook.
        mana_pool: ManaPool::EMPTY,
        spells_cast: rng.between(0, 2) as u8,
        cards_drawn: rng.between(1, 2) as u8,
        land_drop: rng.chance(60),
        ..PlayerFile::default()
    };

    let mut opponent = PlayerFile::default();
    let held = match rng.below(100) {
        0..=34 => 0,
        35..=79 => 1,
        _ => 2,
    };
    for _ in 0..held {
        add_from(rng, &INTERACTION, &mut opponent.hand, &[]);
    }
    let lands = if held == 0 { 0 } else { rng.between(1, 3) };
    for _ in 0..lands {
        opponent
            .battlefield
            .push(PermanentFile::Name(rng.pick(&THEIR_LANDS)));
    }
    if rng.chance(25) {
        opponent
            .battlefield
            .push(PermanentFile::Name(rng.pick(&HATE)));
    }

    PuzzleFile {
        id: None,
        title: None,
        you,
        opponent,
        solution: None,
        attempts: None,
        difficulty: None,
        guest: None,
    }
}

/// How the generator searches.
#[derive(Clone, Debug, PartialEq)]
pub struct GenerateOptions {
    /// The full rating of a puzzle that got past the screen.
    pub rate: RateOptions,
    /// The per-pile budget for the quick screen that throws away deals where
    /// no pile wins.
    pub screen_budget: usize,
    /// The bar a kept puzzle must clear.
    pub criteria: Criteria,
    /// Keep only puzzles of this difficulty.
    pub difficulty: Option<Difficulty>,
}

impl Default for GenerateOptions {
    fn default() -> GenerateOptions {
        GenerateOptions {
            rate: RateOptions::default(),
            screen_budget: 5_000,
            criteria: Criteria::default(),
            difficulty: None,
        }
    }
}

/// A dealt puzzle and what became of it.
#[derive(Clone, Debug, PartialEq)]
pub enum Attempt {
    /// It cleared the bar: the file (with id, title and a solution) and its rating.
    Kept(Box<PuzzleFile>, Box<Rating>),
    /// No pile won in the quick screen.
    Screened,
    /// It was rated but fell short; the reasons.
    Rejected(Vec<String>),
}

/// A key that is equal for two files describing the same game state, whatever
/// their id, title, solution or the order their zones are listed in. Used to
/// keep a generator run from saving a puzzle that already exists.
pub fn fingerprint(file: &PuzzleFile) -> String {
    let mut bare = file.clone();
    bare.id = None;
    bare.title = None;
    bare.solution = None;
    bare.difficulty = None;
    bare.attempts = None;
    // The candidates have no order before the pile is chosen; `State::new`
    // sorts the other zones.
    bare.you.library.sort();
    bare.opponent.library.sort();
    format!("{:?}", crate::state::State::new(bare.puzzle()))
}

/// Deals and judges one puzzle. `seed` and `attempt` name it: the same pair
/// always deals the same puzzle, and the id records them (`gen-<seed>-<attempt>`).
pub fn attempt(seed: u64, attempt: u64, options: &GenerateOptions) -> Result<Attempt, EngineError> {
    let mut rng = Rng::new(seed ^ attempt.wrapping_mul(0xD1B5_4A32_D192_ED03));
    let mut file = deal(&mut rng);
    let puzzle = file.puzzle();

    let screen = rate(
        &puzzle,
        &RateOptions {
            per_pile_budget: options.screen_budget,
            opponent_check: false,
            ..options.rate
        },
    )?;
    if screen.winning_sets.is_empty() {
        return Ok(Attempt::Screened);
    }

    let rating = rate(&puzzle, &options.rate)?;
    let mut reasons = rating.check(&options.criteria);
    if let Some(wanted) = options.difficulty
        && rating.difficulty() != Some(wanted)
    {
        reasons.push(format!("not {wanted}"));
    }
    if !reasons.is_empty() {
        return Ok(Attempt::Rejected(reasons));
    }
    let name = format!("gen-{seed}-{attempt}");
    let difficulty = rating.difficulty();
    file.title = Some(match difficulty {
        Some(tier) => format!("Generated puzzle {seed}-{attempt} ({tier})"),
        None => format!("Generated puzzle {seed}-{attempt}"),
    });
    file.difficulty = difficulty;
    file.id = Some(name);
    file.solution = rating
        .winning_sets
        .first()
        .and_then(|set| set.orderings.first())
        .cloned();
    Ok(Attempt::Kept(Box::new(file), Box::new(rating)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::{Verdict, verify};

    #[test]
    fn the_same_seed_deals_the_same_puzzle() {
        let a = deal(&mut Rng::new(7));
        let b = deal(&mut Rng::new(7));
        let c = deal(&mut Rng::new(8));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn life_costs_are_only_dealt_when_payable() {
        assert_eq!(after_doomsday(4), 2);
        assert_eq!(after_doomsday(7), 3);
        assert!(!affordable(Card::StreetWraith, 2));
        assert!(affordable(Card::StreetWraith, 3));
        assert!(affordable(Card::GitaxianProbe, 1));
        assert!(!affordable(Card::PollutedDelta, 1));
        assert!(!payable(&DRAW, 2).contains(&Card::NightsWhisper));
        assert!(payable(&DRAW, 2).contains(&Card::GitaxianProbe));
    }

    #[test]
    fn fingerprints_ignore_names_and_zone_order() {
        let a = deal(&mut Rng::new(3));
        let mut b = a.clone();
        b.id = Some("other".to_string());
        b.title = Some("Other".to_string());
        b.solution = Some(vec![Card::Island]);
        b.difficulty = Some(Difficulty::Hard);
        b.you.library.reverse();
        assert_eq!(fingerprint(&a), fingerprint(&b));
        let c = deal(&mut Rng::new(4));
        assert_ne!(fingerprint(&a), fingerprint(&c));
    }

    #[test]
    fn rng_ranges_stay_in_bounds() {
        let mut rng = Rng::new(1);
        for _ in 0..1000 {
            assert!(rng.below(3) < 3);
            let n = rng.between(7, 9);
            assert!((7..=9).contains(&n));
        }
    }

    #[test]
    fn every_deal_is_legal_and_within_the_limits() {
        for seed in 0..300 {
            let file = deal(&mut Rng::new(seed));
            let json = file.to_json().expect("serializes");
            let back = PuzzleFile::parse(&json).expect("parses");
            assert_eq!(back, file, "seed {seed}");
            let candidates = file.you.library.len() + file.you.graveyard.len();
            assert!(
                (7..=9).contains(&candidates),
                "seed {seed}: {candidates} candidates"
            );
            assert!(file.opponent.hand.len() <= 3, "seed {seed}");
            let their_sources = file
                .opponent
                .battlefield
                .iter()
                .filter(|p| {
                    let card = match p {
                        PermanentFile::Name(card) => *card,
                        PermanentFile::Full(entry) => entry.card,
                    };
                    !HATE.contains(&card)
                })
                .count();
            assert!(their_sources <= 3, "seed {seed}");
            assert!(file.you.mana_pool.is_empty(), "seed {seed}: floating mana");
            let left = after_doomsday(file.you.life);
            for card in file
                .you
                .hand
                .iter()
                .chain(&file.you.library)
                .chain(&file.you.graveyard)
            {
                assert!(
                    affordable(*card, left),
                    "seed {seed}: {card} at {left} life"
                );
            }
            let mut all = file.you.library.clone();
            all.extend(&file.you.graveyard);
            for card in &all {
                let copies = all.iter().filter(|c| *c == card).count();
                assert!(copies <= MAX_COPIES, "seed {seed}: {copies} x {card}");
            }
        }
    }

    /// Rates thousands of piles per dealt puzzle, which takes minutes in a
    /// debug build. Run it with `cargo test --release -- --ignored`.
    #[test]
    #[ignore = "slow without optimisations: cargo test --release -- --ignored"]
    fn a_kept_puzzle_clears_the_bar_and_its_solution_wins() {
        let options = GenerateOptions {
            rate: RateOptions {
                per_pile_budget: 20_000,
                ..RateOptions::default()
            },
            screen_budget: 2_000,
            criteria: Criteria::default(),
            difficulty: None,
        };
        let seed = 2026;
        for n in 0..200 {
            if let Attempt::Kept(file, rating) = attempt(seed, n, &options).expect("runs") {
                assert!(rating.check(&options.criteria).is_empty());
                assert_eq!(file.difficulty, rating.difficulty());
                assert!(file.difficulty.is_some());
                assert_eq!(file.id.as_deref(), Some(format!("gen-{seed}-{n}").as_str()));
                let pile = file.solution.clone().expect("a solution");
                assert_eq!(pile.len(), 5);
                assert!(matches!(
                    verify(file.puzzle(), &pile),
                    Ok(Verdict::Wins { .. })
                ));
                return;
            }
        }
        panic!("no puzzle kept in 200 attempts");
    }
}
