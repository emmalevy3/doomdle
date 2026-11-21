//! The JSON puzzle file format.
//!
//! A file describes the game as Doomsday resolves, with cards by their
//! printed names, and optionally the intended solution (the pile, top
//! first). The file structs here mirror a [`Puzzle`], not the engine's
//! `State`: zones and flags that can't be set when Doomsday resolves (exile,
//! Reckless Impulse's cards, Silence, Veil, an unordered library) have no
//! field on purpose.
//!
//! ```json
//! {
//!   "id": "2026-01-05",
//!   "title": "Cavern keeps the counter out",
//!   "you": {
//!     "life": 20,
//!     "hand": ["Thassa's Oracle"],
//!     "battlefield": ["Island", {"card": "Cavern of Souls", "type": "Wizard"}],
//!     "library": ["Swamp"],
//!     "cards_drawn": 1
//!   },
//!   "opponent": { "hand": ["Counterspell"], "battlefield": ["Island", "Island"] },
//!   "solution": ["Swamp"]
//! }
//! ```

use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::cards::{Card, CreatureType};
use crate::mana::{Color, ManaPool};
use crate::quality::Difficulty;
use crate::state::{Permanent, PlayerState, Puzzle};

/// A puzzle as written in a file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PuzzleFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub you: PlayerFile,
    /// Missing: a passive opponent with nothing in hand or in play.
    #[serde(default)]
    pub opponent: PlayerFile,
    /// The intended winning pile, top first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solution: Option<Vec<Card>>,
    /// How many piles a player may try on this puzzle; missing means the
    /// site's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempts: Option<u8>,
    /// How hard the puzzle is ("easy", "medium" or "hard"), from its rating;
    /// stored because the browser can't afford to rate a puzzle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<Difficulty>,
}

/// One player's side. Every field has a default: 20 life, empty zones, no
/// mana floating, nothing cast or drawn yet, the land drop still available.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerFile {
    /// For you, the total *before* Doomsday's life loss.
    #[serde(default = "twenty")]
    pub life: i16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hand: Vec<Card>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub battlefield: Vec<PermanentFile>,
    /// For you, the cards the pile may be built from (with the graveyard),
    /// in no particular order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub library: Vec<Card>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graveyard: Vec<Card>,
    /// Mana floating as Doomsday resolves, as a string of symbols: "UUB",
    /// "CC".
    #[serde(
        default,
        skip_serializing_if = "ManaPool::is_empty",
        with = "mana_pool"
    )]
    pub mana_pool: ManaPool,
    /// Spells cast this turn before Doomsday.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub spells_cast: u8,
    /// Cards drawn this turn, the draw step included.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cards_drawn: u8,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub land_drop: bool,
    /// Yawgmoth's Will already resolved this turn.
    #[serde(default, skip_serializing_if = "is_false")]
    pub yawgmoths_will: bool,
}

impl Default for PlayerFile {
    fn default() -> PlayerFile {
        PlayerFile {
            life: 20,
            hand: Vec::new(),
            battlefield: Vec::new(),
            library: Vec::new(),
            graveyard: Vec::new(),
            mana_pool: ManaPool::EMPTY,
            spells_cast: 0,
            cards_drawn: 0,
            land_drop: true,
            yawgmoths_will: false,
        }
    }
}

/// A permanent in a file: a bare card name for an untapped permanent that
/// has been there since before this turn, or an object with the details.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermanentFile {
    Name(Card),
    Full(PermanentEntry),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermanentEntry {
    pub card: Card,
    #[serde(default, skip_serializing_if = "is_false")]
    pub tapped: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub summoning_sick: bool,
    /// A planeswalker's loyalty; missing means its starting loyalty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loyalty: Option<u8>,
    /// Cavern of Souls' chosen creature type.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub creature_type: Option<CreatureType>,
    /// A creature wearing Lightning Greaves.
    #[serde(default, skip_serializing_if = "is_false")]
    pub greaves: bool,
    /// A creature wearing Skateboard.
    #[serde(default, skip_serializing_if = "is_false")]
    pub skateboard: bool,
}

/// A file that couldn't be read: malformed JSON, an unknown field, or a
/// name that matches no card or creature type.
#[derive(Debug)]
pub struct FormatError(serde_json::Error);

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "puzzle file: {}", self.0)
    }
}

impl std::error::Error for FormatError {}

impl PuzzleFile {
    pub fn parse(text: &str) -> Result<PuzzleFile, FormatError> {
        let file: PuzzleFile = serde_json::from_str(text).map_err(FormatError)?;
        // A Treasure is a token: it only exists on the battlefield.
        for (who, player) in [("you", &file.you), ("opponent", &file.opponent)] {
            let zones = [
                ("hand", &player.hand),
                ("library", &player.library),
                ("graveyard", &player.graveyard),
            ];
            for (zone, cards) in zones {
                if cards.contains(&Card::Treasure) {
                    return Err(FormatError(de::Error::custom(format!(
                        "Treasure is a token and can only be on the battlefield, \
                         not in {who}'s {zone}"
                    ))));
                }
            }
        }
        Ok(file)
    }

    pub fn to_json(&self) -> Result<String, FormatError> {
        serde_json::to_string_pretty(self).map_err(FormatError)
    }

    /// The puzzle this file describes.
    pub fn puzzle(&self) -> Puzzle {
        Puzzle {
            you: self.you.player_state(),
            opponent: self.opponent.player_state(),
        }
    }

    /// A file describing `puzzle`, with bare names wherever the details are
    /// the defaults.
    pub fn from_puzzle(puzzle: &Puzzle, solution: Option<Vec<Card>>) -> PuzzleFile {
        PuzzleFile {
            id: None,
            title: None,
            you: PlayerFile::from_player_state(&puzzle.you),
            opponent: PlayerFile::from_player_state(&puzzle.opponent),
            solution,
            attempts: None,
            difficulty: None,
        }
    }
}

impl PlayerFile {
    fn player_state(&self) -> PlayerState {
        PlayerState {
            life: self.life,
            hand: self.hand.clone(),
            library: self.library.clone(),
            graveyard: self.graveyard.clone(),
            battlefield: self
                .battlefield
                .iter()
                .map(PermanentFile::permanent)
                .collect(),
            mana_pool: self.mana_pool,
            spells_cast_this_turn: self.spells_cast,
            cards_drawn_this_turn: self.cards_drawn,
            land_drop_available: self.land_drop,
            yawgmoths_will: self.yawgmoths_will,
            ..PlayerState::default()
        }
    }

    fn from_player_state(ps: &PlayerState) -> PlayerFile {
        PlayerFile {
            life: ps.life,
            hand: ps.hand.clone(),
            battlefield: ps
                .battlefield
                .iter()
                .map(PermanentFile::from_permanent)
                .collect(),
            library: ps.library.clone(),
            graveyard: ps.graveyard.clone(),
            mana_pool: ps.mana_pool,
            spells_cast: ps.spells_cast_this_turn,
            cards_drawn: ps.cards_drawn_this_turn,
            land_drop: ps.land_drop_available,
            yawgmoths_will: ps.yawgmoths_will,
        }
    }
}

impl PermanentFile {
    fn permanent(&self) -> Permanent {
        match self {
            PermanentFile::Name(card) => Permanent::untapped(*card),
            PermanentFile::Full(entry) => Permanent {
                tapped: entry.tapped,
                summoning_sick: entry.summoning_sick,
                loyalty: entry.loyalty.unwrap_or(entry.card.starting_loyalty()),
                chosen_type: entry.creature_type,
                greaves: entry.greaves,
                skateboard: entry.skateboard,
                ..Permanent::untapped(entry.card)
            },
        }
    }

    fn from_permanent(perm: &Permanent) -> PermanentFile {
        if *perm == Permanent::untapped(perm.card) {
            return PermanentFile::Name(perm.card);
        }
        PermanentFile::Full(PermanentEntry {
            card: perm.card,
            tapped: perm.tapped,
            summoning_sick: perm.summoning_sick,
            loyalty: (perm.loyalty != perm.card.starting_loyalty()).then_some(perm.loyalty),
            creature_type: perm.chosen_type,
            greaves: perm.greaves,
            skateboard: perm.skateboard,
        })
    }
}

impl Serialize for PermanentFile {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            PermanentFile::Name(card) => card.serialize(serializer),
            PermanentFile::Full(entry) => entry.serialize(serializer),
        }
    }
}

/// A string is a bare name, an object is a full entry. Done by hand rather
/// than with `#[serde(untagged)]` so a wrong card name is reported as such
/// instead of "data did not match any variant".
impl<'de> Deserialize<'de> for PermanentFile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<PermanentFile, D::Error> {
        struct PermanentVisitor;

        impl<'de> Visitor<'de> for PermanentVisitor {
            type Value = PermanentFile;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a card name or an object with a \"card\" field")
            }

            fn visit_str<E: de::Error>(self, name: &str) -> Result<PermanentFile, E> {
                name.parse().map(PermanentFile::Name).map_err(E::custom)
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<PermanentFile, M::Error> {
                PermanentEntry::deserialize(de::value::MapAccessDeserializer::new(map))
                    .map(PermanentFile::Full)
            }
        }

        deserializer.deserialize_any(PermanentVisitor)
    }
}

/// A mana pool as a string of symbols, "WUBRG" plus "C" for colorless.
mod mana_pool {
    use super::*;

    pub fn serialize<S: Serializer>(pool: &ManaPool, serializer: S) -> Result<S::Ok, S::Error> {
        let mut text = String::new();
        for color in Color::ALL {
            text.extend(std::iter::repeat_n(
                color.symbol(),
                usize::from(pool.get(color)),
            ));
        }
        text.extend(std::iter::repeat_n('C', usize::from(pool.get_colorless())));
        serializer.serialize_str(&text)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<ManaPool, D::Error> {
        let text = String::deserialize(deserializer)?;
        let mut pool = ManaPool::EMPTY;
        for symbol in text.chars() {
            match Color::ALL.iter().find(|c| c.symbol() == symbol) {
                Some(&color) => pool.add_mana(color, 1),
                None if symbol == 'C' => pool.add_colorless(1),
                None => {
                    return Err(de::Error::custom(format!(
                        "unknown mana symbol {symbol:?} in {text:?}"
                    )));
                }
            }
        }
        Ok(pool)
    }
}

fn twenty() -> i16 {
    20
}

fn yes() -> bool {
    true
}

fn is_zero(n: &u8) -> bool {
    *n == 0
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mana::Color;

    const EXAMPLE: &str = r#"{
  "id": "2026-01-05",
  "title": "Cavern keeps the counter out",
  "you": {
    "life": 12,
    "hand": ["Thassa's Oracle"],
    "battlefield": [
      "Island",
      {"card": "Cavern of Souls", "type": "Wizard"},
      {"card": "Doctor Doom, Unrivaled", "tapped": true, "summoning_sick": true},
      {"card": "Teferi, Time Raveler", "loyalty": 3},
      {"card": "Thassa's Oracle", "greaves": true}
    ],
    "library": ["Swamp", "Brainstorm"],
    "graveyard": ["Lotus Petal"],
    "mana_pool": "UUBC",
    "spells_cast": 1,
    "cards_drawn": 2,
    "land_drop": false,
    "yawgmoths_will": true
  },
  "opponent": {
    "hand": ["Counterspell"],
    "battlefield": ["Island", "Island"]
  },
  "solution": ["Swamp", "Brainstorm"],
  "attempts": 5,
  "difficulty": "hard"
}"#;

    #[test]
    fn example_parses_into_the_right_puzzle() {
        let file = PuzzleFile::parse(EXAMPLE).unwrap();
        assert_eq!(file.id.as_deref(), Some("2026-01-05"));
        assert_eq!(file.attempts, Some(5));
        assert_eq!(file.difficulty, Some(Difficulty::Hard));
        assert_eq!(file.solution, Some(vec![Card::Swamp, Card::Brainstorm]));
        let puzzle = file.puzzle();
        let you = &puzzle.you;
        assert_eq!(you.life, 12);
        assert_eq!(you.hand, vec![Card::ThassasOracle]);
        assert_eq!(you.library, vec![Card::Swamp, Card::Brainstorm]);
        assert_eq!(you.graveyard, vec![Card::LotusPetal]);
        assert_eq!(
            you.mana_pool,
            ManaPool::of(Color::U, 2)
                .with(Color::B, 1)
                .with_colorless(1)
        );
        assert_eq!(you.spells_cast_this_turn, 1);
        assert_eq!(you.cards_drawn_this_turn, 2);
        assert!(!you.land_drop_available);
        assert!(you.yawgmoths_will);
        assert!(you.library_ordered);
        assert!(you.exile.is_empty() && !you.veiled && !you.silenced);
        assert_eq!(you.battlefield[0], Permanent::untapped(Card::Island));
        assert_eq!(you.battlefield[1], Permanent::cavern(CreatureType::Wizard));
        let doom = you.battlefield[2];
        assert!(doom.tapped && doom.summoning_sick);
        assert_eq!(you.battlefield[3].loyalty, 3);
        assert!(you.battlefield[4].greaves);
        let opponent = &puzzle.opponent;
        assert_eq!(opponent.life, 20);
        assert_eq!(opponent.hand, vec![Card::Counterspell]);
        assert_eq!(opponent.battlefield.len(), 2);
        assert!(opponent.land_drop_available);
    }

    #[test]
    fn only_you_is_required_and_everything_defaults() {
        let file = PuzzleFile::parse(r#"{"you": {}}"#).unwrap();
        assert_eq!(
            file.puzzle(),
            Puzzle {
                you: PlayerState::default(),
                opponent: PlayerState::default(),
            }
        );
        assert_eq!(file.solution, None);
        assert_eq!(file.attempts, None);
        assert_eq!(file.difficulty, None);
    }

    #[test]
    fn an_unknown_difficulty_is_an_error() {
        let err = PuzzleFile::parse(r#"{"you": {}, "difficulty": "brutal"}"#).unwrap_err();
        assert!(err.to_string().contains("brutal"), "{err}");
    }

    #[test]
    fn round_trips_through_json() {
        let file = PuzzleFile::parse(EXAMPLE).unwrap();
        let again = PuzzleFile::parse(&file.to_json().unwrap()).unwrap();
        assert_eq!(again, file);
        // And from a puzzle built in code: defaults become bare names.
        let rebuilt = PuzzleFile::from_puzzle(&file.puzzle(), file.solution.clone());
        assert_eq!(rebuilt.puzzle(), file.puzzle());
        assert_eq!(
            rebuilt.you.battlefield[0],
            PermanentFile::Name(Card::Island)
        );
        assert!(matches!(
            rebuilt.you.battlefield[1],
            PermanentFile::Full(PermanentEntry {
                card: Card::CavernOfSouls,
                creature_type: Some(CreatureType::Wizard),
                ..
            })
        ));
        let text = rebuilt.to_json().unwrap();
        assert!(text.contains("\"mana_pool\": \"UUBC\""));
        assert!(!text.contains("\"id\""));
    }

    #[test]
    fn errors_name_the_problem() {
        let unknown = PuzzleFile::parse(r#"{"you": {"hand": ["Thassa's Oracel"]}}"#)
            .unwrap_err()
            .to_string();
        assert!(unknown.contains("Thassa's Oracel"), "{unknown}");
        let bad_permanent = PuzzleFile::parse(r#"{"you": {"battlefield": ["Islnad"]}}"#)
            .unwrap_err()
            .to_string();
        assert!(bad_permanent.contains("Islnad"), "{bad_permanent}");
        let bad_type = PuzzleFile::parse(
            r#"{"you": {"battlefield": [{"card": "Cavern of Souls", "type": "Wizzard"}]}}"#,
        )
        .unwrap_err()
        .to_string();
        assert!(bad_type.contains("Wizzard"), "{bad_type}");
        let bad_field = PuzzleFile::parse(r#"{"you": {"exile": []}}"#)
            .unwrap_err()
            .to_string();
        assert!(bad_field.contains("exile"), "{bad_field}");
        let bad_mana = PuzzleFile::parse(r#"{"you": {"mana_pool": "UX"}}"#)
            .unwrap_err()
            .to_string();
        assert!(bad_mana.contains("'X'"), "{bad_mana}");
        assert!(PuzzleFile::parse("{").is_err());
    }

    #[test]
    fn a_treasure_is_a_token_and_only_on_the_battlefield() {
        let file = PuzzleFile::parse(
            r#"{"you": {"hand": ["An Offer You Can't Refuse"], "battlefield": ["Treasure"]},
                "opponent": {"battlefield": ["Treasure"]}}"#,
        )
        .unwrap();
        let puzzle = file.puzzle();
        assert!(puzzle.you.battlefield[0].token);
        assert!(puzzle.opponent.battlefield[0].token);
        assert_eq!(puzzle.you.hand, vec![Card::AnOfferYouCantRefuse]);
        // Written back as a bare name.
        let json = PuzzleFile::from_puzzle(&puzzle, None).to_json().unwrap();
        assert!(json.contains("\"Treasure\""), "{json}");
        for zone in ["hand", "library", "graveyard"] {
            let err = PuzzleFile::parse(&format!(r#"{{"you": {{"{zone}": ["Treasure"]}}}}"#))
                .unwrap_err()
                .to_string();
            assert!(err.contains("Treasure") && err.contains(zone), "{err}");
        }
        let err = PuzzleFile::parse(r#"{"you": {}, "opponent": {"hand": ["Treasure"]}}"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("opponent's hand"), "{err}");
    }

    #[test]
    fn every_card_and_creature_type_parses_by_name() {
        for card in Card::ALL {
            assert_eq!(card.name().parse::<Card>(), Ok(card));
        }
        for creature_type in CreatureType::ALL {
            assert_eq!(
                creature_type.to_string().parse::<CreatureType>(),
                Ok(creature_type)
            );
        }
        assert_eq!(
            "Nope".parse::<Card>(),
            Err(crate::cards::UnknownName("Nope".to_string()))
        );
    }
}
