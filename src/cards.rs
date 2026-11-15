//! The card pool and every card-specific behavior, as `match` arms.
//!
//! Only cards from `CARDS.md` belong here. Stages 1–4a cover Thassa's Oracle,
//! Jace, Doctor Doom, Laboratory Maniac, Doomsday, the blue cantrips, Probe,
//! Street Wraith, Edge of Autumn, the graveyard package (Unearth, Yawgmoth's
//! Will, Flow State, Cabal Ritual, Frantic Search), the fast mana, the
//! basic, dual, fetch and surveil lands, and the counterspells.

use std::fmt;

use crate::engine::{
    Choice, EngineError, multisets, ordered_selections, pay_mana, tax_payments,
    unordered_selections,
};
use crate::mana::{AltCost, Color, ManaCost, Payment};
use crate::state::{
    ChoiceKind, LossReason, PendingChoice, Player, PlayerState, SpellMode, StackItem, State, Target,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Card {
    // Win conditions
    ThassasOracle,
    JaceWielderOfMysteries,
    DoctorDoom,
    LaboratoryManiac,
    // Doomsday
    Doomsday,
    // Graveyard
    Unearth,
    YawgmothsWill,
    // Counterspells (either player's)
    Counterspell,
    ForceOfWill,
    ForceOfNegation,
    Daze,
    SpellPierce,
    Miscalculation,
    MentalMisstep,
    SpellSnare,
    Stifle,
    PactOfNegation,
    Flusterstorm,
    ConsignToMemory,
    MindbreakTrap,
    Pyroblast,
    Hydroblast,
    // Removal (either player's)
    SwordsToPlowshares,
    SnuffOut,
    SurgicalExtraction,
    // Your protection and discard
    Silence,
    VeilOfSummer,
    Teferi,
    DefenseGrid,
    Thoughtseize,
    Duress,
    Grief,
    // The opponent's board (static hate)
    Narset,
    OrcishBowmasters,
    LeylineOfTheVoid,
    GrafdiggersCage,
    Thalia,
    ArchonOfEmeria,
    // Draw and selection
    Brainstorm,
    Ponder,
    Preordain,
    Consider,
    AncestralRecall,
    GitaxianProbe,
    StreetWraith,
    EdgeOfAutumn,
    FlowState,
    FranticSearch,
    // Mana
    DarkRitual,
    CabalRitual,
    LionsEyeDiamond,
    BlackLotus,
    LotusPetal,
    MoxSapphire,
    MoxJet,
    // Basic lands
    Plains,
    Island,
    Swamp,
    Mountain,
    Forest,
    // Original dual lands
    UndergroundSea,
    Tundra,
    VolcanicIsland,
    TropicalIsland,
    Scrubland,
    Badlands,
    Bayou,
    Plateau,
    Savannah,
    Taiga,
    // Other lands
    UndercitySewers,
    MeticulousArchive,
    PollutedDelta,
    FloodedStrand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CardKind {
    Instant,
    Sorcery,
    Creature,
    Artifact,
    Enchantment,
    Planeswalker,
    Land,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LandType {
    Plains,
    Island,
    Swamp,
    Mountain,
    Forest,
}

impl LandType {
    pub const fn color(self) -> Color {
        match self {
            LandType::Plains => Color::W,
            LandType::Island => Color::U,
            LandType::Swamp => Color::B,
            LandType::Mountain => Color::R,
            LandType::Forest => Color::G,
        }
    }
}

/// How a permanent makes mana. Mana abilities never use the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ManaAbility {
    /// {T}: add one mana of one of the colors from [`Card::mana_colors`].
    Tap,
    /// {T}, sacrifice: add this many mana of any one color.
    Sacrifice(u8),
    /// Lion's Eye Diamond: discard your hand, sacrifice: add three mana of
    /// one color. Activate only as an instant.
    Led,
}

/// The cost of a card's cycling ability (discarding the card is implied).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CyclingCost {
    Mana(ManaCost),
    Life(u8),
    SacrificeLand,
}

/// What a library search may find.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LandFilter {
    /// A land with at least one of these subtypes (fetchlands).
    Types(&'static [LandType]),
    /// A basic land (Edge of Autumn).
    Basic,
}

impl LandFilter {
    pub fn matches(self, card: Card) -> bool {
        match self {
            LandFilter::Types(types) => card.land_types().iter().any(|t| types.contains(t)),
            LandFilter::Basic => card.is_basic_land(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TriggerKind {
    /// Thassa's Oracle's enter trigger.
    OracleEnters,
    /// Undercity Sewers / Meticulous Archive: surveil 1 on entering.
    Surveil,
    /// Storm (Flusterstorm) or replicate (Consign to Memory): put `count`
    /// copies of the spell directly below this trigger onto the stack.
    Copies { count: u8 },
    /// Grief enters: the opponent discards a nonland card of your choice.
    GriefEnters,
    /// Orcish Bowmasters: the other player drew a card; deal 1 damage to a
    /// target of its controller's choice.
    BowmastersPing,
}

/// An activated ability on the stack, costs already paid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AbilityKind {
    /// Draw a card.
    Cycling,
    /// Search for a land matching the source's [`Card::fetch_filter`].
    Fetch,
    /// Doctor Doom: draw, lose 1 life, win if the library is empty.
    DoomDraw,
    /// Jace +1: target player mills two, then you draw.
    JacePlusOne { target: Player },
    /// Teferi +1: your sorceries have flash until your next turn.
    TeferiPlusOne,
    /// Teferi −3: return up to one target artifact, creature or enchantment
    /// to its owner's hand, then draw a card.
    TeferiMinusThree { target: Option<Target> },
}

/// A loyalty ability a planeswalker can activate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoyaltyAbility {
    JacePlusOne { target: Player },
    TeferiPlusOne,
    TeferiMinusThree { target: Option<Target> },
}

impl LoyaltyAbility {
    /// Loyalty added (or, negative, removed) as the cost.
    pub fn loyalty_change(self) -> i8 {
        match self {
            LoyaltyAbility::JacePlusOne { .. } | LoyaltyAbility::TeferiPlusOne => 1,
            LoyaltyAbility::TeferiMinusThree { .. } => -3,
        }
    }

    pub fn ability(self) -> AbilityKind {
        match self {
            LoyaltyAbility::JacePlusOne { target } => AbilityKind::JacePlusOne { target },
            LoyaltyAbility::TeferiPlusOne => AbilityKind::TeferiPlusOne,
            LoyaltyAbility::TeferiMinusThree { target } => AbilityKind::TeferiMinusThree { target },
        }
    }
}

impl Card {
    pub fn name(self) -> &'static str {
        match self {
            Card::ThassasOracle => "Thassa's Oracle",
            Card::JaceWielderOfMysteries => "Jace, Wielder of Mysteries",
            Card::DoctorDoom => "Doctor Doom, Unrivaled",
            Card::LaboratoryManiac => "Laboratory Maniac",
            Card::Doomsday => "Doomsday",
            Card::Unearth => "Unearth",
            Card::YawgmothsWill => "Yawgmoth's Will",
            Card::Counterspell => "Counterspell",
            Card::ForceOfWill => "Force of Will",
            Card::ForceOfNegation => "Force of Negation",
            Card::Daze => "Daze",
            Card::SpellPierce => "Spell Pierce",
            Card::Miscalculation => "Miscalculation",
            Card::MentalMisstep => "Mental Misstep",
            Card::SpellSnare => "Spell Snare",
            Card::Stifle => "Stifle",
            Card::PactOfNegation => "Pact of Negation",
            Card::Flusterstorm => "Flusterstorm",
            Card::ConsignToMemory => "Consign to Memory",
            Card::MindbreakTrap => "Mindbreak Trap",
            Card::Pyroblast => "Pyroblast",
            Card::Hydroblast => "Hydroblast",
            Card::SwordsToPlowshares => "Swords to Plowshares",
            Card::SnuffOut => "Snuff Out",
            Card::SurgicalExtraction => "Surgical Extraction",
            Card::Silence => "Silence",
            Card::VeilOfSummer => "Veil of Summer",
            Card::Teferi => "Teferi, Time Raveler",
            Card::DefenseGrid => "Defense Grid",
            Card::Thoughtseize => "Thoughtseize",
            Card::Duress => "Duress",
            Card::Grief => "Grief",
            Card::Narset => "Narset, Parter of Veils",
            Card::OrcishBowmasters => "Orcish Bowmasters",
            Card::LeylineOfTheVoid => "Leyline of the Void",
            Card::GrafdiggersCage => "Grafdigger's Cage",
            Card::Thalia => "Thalia, Guardian of Thraben",
            Card::ArchonOfEmeria => "Archon of Emeria",
            Card::Brainstorm => "Brainstorm",
            Card::Ponder => "Ponder",
            Card::Preordain => "Preordain",
            Card::Consider => "Consider",
            Card::AncestralRecall => "Ancestral Recall",
            Card::GitaxianProbe => "Gitaxian Probe",
            Card::StreetWraith => "Street Wraith",
            Card::EdgeOfAutumn => "Edge of Autumn",
            Card::FlowState => "Flow State",
            Card::FranticSearch => "Frantic Search",
            Card::DarkRitual => "Dark Ritual",
            Card::CabalRitual => "Cabal Ritual",
            Card::LionsEyeDiamond => "Lion's Eye Diamond",
            Card::BlackLotus => "Black Lotus",
            Card::LotusPetal => "Lotus Petal",
            Card::MoxSapphire => "Mox Sapphire",
            Card::MoxJet => "Mox Jet",
            Card::Plains => "Plains",
            Card::Island => "Island",
            Card::Swamp => "Swamp",
            Card::Mountain => "Mountain",
            Card::Forest => "Forest",
            Card::UndergroundSea => "Underground Sea",
            Card::Tundra => "Tundra",
            Card::VolcanicIsland => "Volcanic Island",
            Card::TropicalIsland => "Tropical Island",
            Card::Scrubland => "Scrubland",
            Card::Badlands => "Badlands",
            Card::Bayou => "Bayou",
            Card::Plateau => "Plateau",
            Card::Savannah => "Savannah",
            Card::Taiga => "Taiga",
            Card::UndercitySewers => "Undercity Sewers",
            Card::MeticulousArchive => "Meticulous Archive",
            Card::PollutedDelta => "Polluted Delta",
            Card::FloodedStrand => "Flooded Strand",
        }
    }

    pub fn kind(self) -> CardKind {
        match self {
            Card::ThassasOracle
            | Card::DoctorDoom
            | Card::LaboratoryManiac
            | Card::StreetWraith
            | Card::Grief
            | Card::OrcishBowmasters
            | Card::Thalia
            | Card::ArchonOfEmeria => CardKind::Creature,
            Card::JaceWielderOfMysteries | Card::Teferi | Card::Narset => CardKind::Planeswalker,
            Card::LeylineOfTheVoid => CardKind::Enchantment,
            Card::Doomsday
            | Card::Unearth
            | Card::YawgmothsWill
            | Card::Ponder
            | Card::Preordain
            | Card::GitaxianProbe
            | Card::EdgeOfAutumn
            | Card::FlowState
            | Card::Thoughtseize
            | Card::Duress => CardKind::Sorcery,
            Card::Brainstorm
            | Card::Consider
            | Card::AncestralRecall
            | Card::FranticSearch
            | Card::DarkRitual
            | Card::CabalRitual
            | Card::Counterspell
            | Card::ForceOfWill
            | Card::ForceOfNegation
            | Card::Daze
            | Card::SpellPierce
            | Card::Miscalculation
            | Card::MentalMisstep
            | Card::SpellSnare
            | Card::Stifle
            | Card::PactOfNegation
            | Card::Flusterstorm
            | Card::ConsignToMemory
            | Card::MindbreakTrap
            | Card::Pyroblast
            | Card::Hydroblast
            | Card::SwordsToPlowshares
            | Card::SnuffOut
            | Card::SurgicalExtraction
            | Card::Silence
            | Card::VeilOfSummer => CardKind::Instant,
            Card::LionsEyeDiamond
            | Card::BlackLotus
            | Card::LotusPetal
            | Card::MoxSapphire
            | Card::MoxJet
            | Card::DefenseGrid
            | Card::GrafdiggersCage => CardKind::Artifact,
            Card::Plains
            | Card::Island
            | Card::Swamp
            | Card::Mountain
            | Card::Forest
            | Card::UndergroundSea
            | Card::Tundra
            | Card::VolcanicIsland
            | Card::TropicalIsland
            | Card::Scrubland
            | Card::Badlands
            | Card::Bayou
            | Card::Plateau
            | Card::Savannah
            | Card::Taiga
            | Card::UndercitySewers
            | Card::MeticulousArchive
            | Card::PollutedDelta
            | Card::FloodedStrand => CardKind::Land,
        }
    }

    pub fn mana_cost(self) -> ManaCost {
        match self {
            Card::ThassasOracle => ManaCost::colored(Color::U, 2),
            Card::JaceWielderOfMysteries => ManaCost::colored(Color::U, 3).with_generic(1),
            Card::DoctorDoom => ManaCost::colored(Color::B, 2).with_generic(2),
            Card::LaboratoryManiac => ManaCost::colored(Color::U, 1).with_generic(2),
            Card::Doomsday => ManaCost::colored(Color::B, 3),
            Card::Unearth => ManaCost::colored(Color::B, 1),
            Card::YawgmothsWill => ManaCost::colored(Color::B, 1).with_generic(2),
            Card::FlowState => ManaCost::colored(Color::U, 1).with_generic(1),
            Card::FranticSearch => ManaCost::colored(Color::U, 1).with_generic(2),
            Card::CabalRitual => ManaCost::colored(Color::B, 1).with_generic(1),
            Card::Brainstorm
            | Card::Ponder
            | Card::Preordain
            | Card::Consider
            | Card::AncestralRecall => ManaCost::colored(Color::U, 1),
            Card::GitaxianProbe => ManaCost::phyrexian(Color::U, 1),
            Card::StreetWraith => ManaCost::colored(Color::B, 2).with_generic(3),
            Card::EdgeOfAutumn => ManaCost::colored(Color::G, 1).with_generic(1),
            Card::DarkRitual => ManaCost::colored(Color::B, 1),
            Card::Counterspell => ManaCost::colored(Color::U, 2),
            Card::ForceOfWill => ManaCost::colored(Color::U, 2).with_generic(3),
            Card::ForceOfNegation => ManaCost::colored(Color::U, 2).with_generic(1),
            Card::Daze | Card::Miscalculation => ManaCost::colored(Color::U, 1).with_generic(1),
            Card::SpellPierce | Card::SpellSnare | Card::Stifle => ManaCost::colored(Color::U, 1),
            Card::MentalMisstep => ManaCost::phyrexian(Color::U, 1),
            Card::Flusterstorm | Card::ConsignToMemory | Card::Hydroblast => {
                ManaCost::colored(Color::U, 1)
            }
            Card::MindbreakTrap => ManaCost::colored(Color::U, 2).with_generic(2),
            Card::Pyroblast => ManaCost::colored(Color::R, 1),
            Card::SwordsToPlowshares => ManaCost::colored(Color::W, 1),
            Card::SnuffOut => ManaCost::colored(Color::B, 1).with_generic(3),
            Card::SurgicalExtraction => ManaCost::phyrexian(Color::B, 1),
            Card::Silence => ManaCost::colored(Color::W, 1),
            Card::VeilOfSummer => ManaCost::colored(Color::G, 1),
            Card::Teferi => ManaCost::colored(Color::W, 1)
                .with(Color::U, 1)
                .with_generic(1),
            Card::DefenseGrid => ManaCost::FREE.with_generic(2),
            Card::Thoughtseize | Card::Duress => ManaCost::colored(Color::B, 1),
            Card::Grief => ManaCost::colored(Color::B, 2).with_generic(2),
            Card::Narset => ManaCost::colored(Color::U, 2).with_generic(1),
            Card::OrcishBowmasters => ManaCost::colored(Color::B, 1).with_generic(1),
            Card::LeylineOfTheVoid => ManaCost::colored(Color::B, 2).with_generic(2),
            Card::GrafdiggersCage => ManaCost::FREE.with_generic(1),
            Card::Thalia => ManaCost::colored(Color::W, 1).with_generic(1),
            Card::ArchonOfEmeria => ManaCost::colored(Color::W, 1).with_generic(2),
            _ => ManaCost::FREE,
        }
    }

    /// A creature's toughness (0 for everything else): how much damage kills
    /// it.
    pub fn toughness(self) -> u8 {
        match self {
            Card::ThassasOracle | Card::ArchonOfEmeria => 3,
            Card::LaboratoryManiac | Card::Grief => 2,
            Card::DoctorDoom | Card::StreetWraith => 4,
            Card::OrcishBowmasters | Card::Thalia => 1,
            _ => 0,
        }
    }

    /// Evoke: the color of card that may be exiled from hand instead of
    /// paying the mana cost; the creature is then sacrificed as it enters.
    pub fn evoke_pitch(self) -> Option<fn(Card) -> bool> {
        match self {
            Card::Grief => Some(Card::is_black),
            _ => None,
        }
    }

    /// Whether the card's cost has a pip of `color`, Phyrexian pips included.
    fn has_pip(self, color: Color) -> bool {
        let cost = self.mana_cost();
        cost.pip(color) > 0 || cost.phyrexian_pip(color) > 0
    }

    /// Blue cards, for Force of Will's and Force of Negation's pitch and
    /// Pyroblast. Pact of Negation costs {0} but is blue by its color
    /// indicator.
    pub fn is_blue(self) -> bool {
        self.has_pip(Color::U) || self == Card::PactOfNegation
    }

    /// Red cards, for Hydroblast.
    pub fn is_red(self) -> bool {
        self.has_pip(Color::R)
    }

    /// Black cards, for Snuff Out's "nonblack".
    pub fn is_black(self) -> bool {
        self.has_pip(Color::B)
    }

    /// Colorless cards, for Consign to Memory: no colored pip and no color
    /// indicator.
    pub fn is_colorless(self) -> bool {
        !Color::ALL.iter().any(|&c| self.has_pip(c)) && self != Card::PactOfNegation
    }

    /// The replicate cost, paid any number of times on casting.
    pub fn replicate_cost(self) -> Option<ManaCost> {
        match self {
            Card::ConsignToMemory => Some(ManaCost::FREE.with_generic(1)),
            _ => None,
        }
    }

    /// Storm (Flusterstorm): copied once per spell cast before it this turn.
    pub fn has_storm(self) -> bool {
        self == Card::Flusterstorm
    }

    /// Whether the engine models casting this card at all. Street Wraith is
    /// cycling-only per CARDS.md; Doomsday is only ever the spell the puzzle
    /// starts with (a second Doomsday from the graveyard under Yawgmoth's Will
    /// is out of scope for now).
    pub fn castable(self) -> bool {
        !matches!(self, Card::StreetWraith | Card::Doomsday)
    }

    /// For a targeted spell, every distinct legal target `player` could pick
    /// right now; `None` for spells without targets. A targeted spell with an
    /// empty list can't be cast. Counters only ever target the other player's
    /// stack items.
    pub fn target_options(self, state: &State, player: Player) -> Option<Vec<Target>> {
        let other = player.other();
        let stack_targets = |pred: &dyn Fn(&StackItem) -> bool| -> Vec<Target> {
            state
                .stack
                .iter()
                .enumerate()
                .filter(|(_, item)| item.controller() == other && pred(item))
                .map(|(index, item)| Target::Stack {
                    index,
                    card: item.card(),
                })
                .collect()
        };
        // Spells to counter. A veiled player's spells can't be countered, so
        // they are not offered (same outcome, fewer dead branches).
        let spell_where = |pred: &dyn Fn(Card) -> bool| -> Vec<Target> {
            if state.player(other).veiled {
                return Vec::new();
            }
            stack_targets(&|item| matches!(item, StackItem::Spell { card, .. } if pred(*card)))
        };
        // Distinct cards among `owner`'s permanents or graveyard that pass.
        // Veil of Summer gives hexproof from blue and from black.
        let permanents_where = |owner: Player, pred: &dyn Fn(Card) -> bool| -> Vec<Target> {
            if owner != player && hexproof_blocks(state, owner, self) {
                return Vec::new();
            }
            let mut cards: Vec<Card> = state
                .player(owner)
                .battlefield
                .iter()
                .map(|p| p.card)
                .filter(|&c| pred(c))
                .collect();
            cards.sort();
            cards.dedup();
            cards
                .into_iter()
                .map(|card| Target::Permanent { owner, card })
                .collect()
        };
        let graveyard_where = |owner: Player, pred: &dyn Fn(Card) -> bool| -> Vec<Target> {
            let mut cards: Vec<Card> = state
                .player(owner)
                .graveyard
                .iter()
                .copied()
                .filter(|&c| pred(c))
                .collect();
            cards.sort();
            cards.dedup();
            cards
                .into_iter()
                .map(|card| Target::Graveyard { owner, card })
                .collect()
        };
        let creature = |c: Card| c.kind() == CardKind::Creature;
        match self {
            // "Target creature card with mana value 3 or less from your graveyard."
            Card::Unearth => Some(graveyard_where(player, &|c| {
                creature(c) && c.mana_cost().mana_value() <= 3
            })),
            Card::Counterspell
            | Card::ForceOfWill
            | Card::Daze
            | Card::Miscalculation
            | Card::PactOfNegation => Some(spell_where(&|_| true)),
            Card::SpellPierce | Card::ForceOfNegation => Some(spell_where(&|c| !creature(c))),
            Card::MentalMisstep => Some(spell_where(&|c| c.mana_cost().mana_value() == 1)),
            Card::SpellSnare => Some(spell_where(&|c| c.mana_cost().mana_value() == 2)),
            Card::Stifle => Some(stack_targets(&|item| {
                matches!(item, StackItem::Ability { .. } | StackItem::Trigger { .. })
            })),
            Card::Flusterstorm => Some(spell_where(&|c| {
                matches!(c.kind(), CardKind::Instant | CardKind::Sorcery)
            })),
            // "Target triggered ability or colorless spell."
            Card::ConsignToMemory => Some(stack_targets(&|item| match item {
                StackItem::Trigger { .. } => true,
                StackItem::Spell { card, .. } => card.is_colorless(),
                StackItem::Ability { .. } => false,
            })),
            // "Any number of target spells": modeled as every spell the other
            // player controls, so the one target recorded is their topmost.
            // Exiling isn't countering, so Veil doesn't protect against it.
            Card::MindbreakTrap => Some(
                stack_targets(&|item| matches!(item, StackItem::Spell { .. }))
                    .pop()
                    .into_iter()
                    .collect(),
            ),
            // "If it's blue/red": only blue/red things are ever offered.
            Card::Pyroblast => {
                let mut targets = spell_where(&Card::is_blue);
                targets.extend(permanents_where(other, &Card::is_blue));
                Some(targets)
            }
            Card::Hydroblast => {
                let mut targets = spell_where(&Card::is_red);
                targets.extend(permanents_where(other, &Card::is_red));
                Some(targets)
            }
            Card::SwordsToPlowshares => Some(permanents_where(other, &creature)),
            Card::SnuffOut => Some(permanents_where(other, &|c| creature(c) && !c.is_black())),
            // "Target card in a graveyard other than a basic land card": only
            // the other player's graveyard is ever worth it.
            Card::SurgicalExtraction => Some(graveyard_where(other, &|c| !c.is_basic_land())),
            _ => None,
        }
    }

    /// Ways to cast this card other than paying its mana cost, as `player`
    /// could pay them right now.
    pub fn alternative_payments(self, state: &State, player: Player) -> Vec<Payment> {
        let ps = state.player(player);
        let pitches = |life: u8, color: fn(Card) -> bool| -> Vec<Payment> {
            // The spell itself is already on the stack when its cost is paid,
            // so one copy in hand can't pitch itself, but a second copy can.
            let mut hand = ps.hand.clone();
            if let Some(pos) = hand.iter().position(|&c| c == self) {
                hand.remove(pos);
            }
            hand.sort();
            hand.dedup();
            hand.into_iter()
                .filter(|&c| color(c))
                .map(|c| Payment {
                    alt: Some(AltCost::Pitch(c)),
                    ..Payment::life(life)
                })
                .collect()
        };
        match self {
            // "Pay 1 life and exile a blue card from your hand."
            Card::ForceOfWill if ps.life >= 1 => pitches(1, Card::is_blue),
            // "Exile a blue card from your hand. Activate only if it's not
            // your turn": it is always your turn, so only the opponent may.
            Card::ForceOfNegation if player == Player::Opponent => pitches(0, Card::is_blue),
            // Evoke: exile a card of the right color from hand.
            Card::Grief => pitches(0, Card::is_black),
            // "Return an Island you control to its owner's hand."
            Card::Daze => {
                let mut islands: Vec<Card> = ps
                    .battlefield
                    .iter()
                    .map(|p| p.card)
                    .filter(|c| c.land_types().contains(&LandType::Island))
                    .collect();
                islands.sort();
                islands.dedup();
                islands
                    .into_iter()
                    .map(|c| Payment {
                        alt: Some(AltCost::ReturnIsland(c)),
                        ..Payment::FREE
                    })
                    .collect()
            }
            // "If you control a Swamp, you may pay 4 life rather than pay
            // this spell's mana cost."
            Card::SnuffOut
                if ps.life >= 4
                    && ps
                        .battlefield
                        .iter()
                        .any(|p| p.card.land_types().contains(&LandType::Swamp)) =>
            {
                vec![Payment::life(4)]
            }
            // "If an opponent cast three or more spells this turn, you may
            // pay {0} rather than pay this spell's mana cost."
            Card::MindbreakTrap if state.player(player.other()).spells_cast_this_turn >= 3 => {
                vec![Payment::FREE]
            }
            _ => Vec::new(),
        }
    }

    /// {U} pips in the mana cost, for Thassa's Oracle.
    pub fn blue_devotion(self) -> u8 {
        self.mana_cost().pip(Color::U)
    }

    pub fn land_types(self) -> &'static [LandType] {
        use LandType::*;
        match self {
            Card::Plains => &[Plains],
            Card::Island => &[Island],
            Card::Swamp => &[Swamp],
            Card::Mountain => &[Mountain],
            Card::Forest => &[Forest],
            Card::UndergroundSea | Card::UndercitySewers => &[Island, Swamp],
            Card::Tundra | Card::MeticulousArchive => &[Plains, Island],
            Card::VolcanicIsland => &[Island, Mountain],
            Card::TropicalIsland => &[Forest, Island],
            Card::Scrubland => &[Plains, Swamp],
            Card::Badlands => &[Swamp, Mountain],
            Card::Bayou => &[Swamp, Forest],
            Card::Plateau => &[Mountain, Plains],
            Card::Savannah => &[Forest, Plains],
            Card::Taiga => &[Mountain, Forest],
            _ => &[],
        }
    }

    pub fn is_basic_land(self) -> bool {
        matches!(
            self,
            Card::Plains | Card::Island | Card::Swamp | Card::Mountain | Card::Forest
        )
    }

    /// Lands that enter the battlefield tapped.
    pub fn enters_tapped(self) -> bool {
        matches!(self, Card::UndercitySewers | Card::MeticulousArchive)
    }

    /// Which land types a fetchland can find.
    pub fn fetch_filter(self) -> Option<&'static [LandType]> {
        use LandType::*;
        match self {
            Card::PollutedDelta => Some(&[Island, Swamp]),
            Card::FloodedStrand => Some(&[Plains, Island]),
            _ => None,
        }
    }

    pub fn cycling_cost(self) -> Option<CyclingCost> {
        match self {
            Card::StreetWraith => Some(CyclingCost::Life(2)),
            Card::EdgeOfAutumn => Some(CyclingCost::SacrificeLand),
            Card::Unearth | Card::Miscalculation => {
                Some(CyclingCost::Mana(ManaCost::FREE.with_generic(2)))
            }
            _ => None,
        }
    }

    pub fn mana_ability(self) -> Option<ManaAbility> {
        match self {
            Card::LionsEyeDiamond => Some(ManaAbility::Led),
            Card::BlackLotus => Some(ManaAbility::Sacrifice(3)),
            Card::LotusPetal => Some(ManaAbility::Sacrifice(1)),
            Card::MoxSapphire | Card::MoxJet => Some(ManaAbility::Tap),
            _ if self.kind() == CardKind::Land && !self.land_types().is_empty() => {
                Some(ManaAbility::Tap)
            }
            _ => None,
        }
    }

    /// Colors a [`ManaAbility::Tap`] permanent can produce. Lands derive this
    /// from their subtypes.
    pub fn mana_colors(self) -> Vec<Color> {
        match self {
            Card::MoxSapphire => vec![Color::U],
            Card::MoxJet => vec![Color::B],
            _ => self.land_types().iter().map(|t| t.color()).collect(),
        }
    }

    pub fn etb_trigger(self) -> Option<TriggerKind> {
        match self {
            Card::ThassasOracle => Some(TriggerKind::OracleEnters),
            Card::UndercitySewers | Card::MeticulousArchive => Some(TriggerKind::Surveil),
            Card::Grief => Some(TriggerKind::GriefEnters),
            _ => None,
        }
    }

    /// Loyalty a planeswalker enters with (0 for everything else).
    pub fn starting_loyalty(self) -> u8 {
        match self {
            Card::JaceWielderOfMysteries | Card::Teferi => 4,
            Card::Narset => 5,
            _ => 0,
        }
    }

    /// The loyalty abilities `player` could activate on this planeswalker
    /// right now, with their targets (loyalty itself is checked on activation).
    pub fn loyalty_abilities(self, state: &State, player: Player) -> Vec<LoyaltyAbility> {
        match self {
            Card::JaceWielderOfMysteries => vec![
                LoyaltyAbility::JacePlusOne {
                    target: Player::You,
                },
                LoyaltyAbility::JacePlusOne {
                    target: Player::Opponent,
                },
            ],
            Card::Teferi => {
                let mut out = vec![
                    LoyaltyAbility::TeferiPlusOne,
                    LoyaltyAbility::TeferiMinusThree { target: None },
                ];
                // "Up to one target artifact, creature, or enchantment",
                // either player's.
                for owner in [player, player.other()] {
                    let mut cards: Vec<Card> = state
                        .player(owner)
                        .battlefield
                        .iter()
                        .map(|p| p.card)
                        .filter(|c| {
                            matches!(
                                c.kind(),
                                CardKind::Artifact | CardKind::Creature | CardKind::Enchantment
                            )
                        })
                        .collect();
                    cards.sort();
                    cards.dedup();
                    out.extend(
                        cards
                            .into_iter()
                            .map(|card| LoyaltyAbility::TeferiMinusThree {
                                target: Some(Target::Permanent { owner, card }),
                            }),
                    );
                }
                out
            }
            _ => Vec::new(),
        }
    }

    /// "If you would draw a card while your library has no cards in it, you
    /// win the game instead."
    pub fn replaces_empty_draw_with_win(self) -> bool {
        matches!(self, Card::LaboratoryManiac | Card::JaceWielderOfMysteries)
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl fmt::Display for AbilityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AbilityKind::Cycling => f.write_str("cycling"),
            AbilityKind::Fetch => f.write_str("fetch"),
            AbilityKind::DoomDraw => f.write_str("draw ability"),
            AbilityKind::JacePlusOne { target } => write!(f, "+1 targeting {target:?}"),
            AbilityKind::TeferiPlusOne => f.write_str("+1"),
            AbilityKind::TeferiMinusThree { target: Some(t) } => write!(f, "-3 targeting {t}"),
            AbilityKind::TeferiMinusThree { target: None } => f.write_str("-3 with no target"),
        }
    }
}

/// Resolves an instant or sorcery. Cards that need a decision set
/// `state.pending` and finish in [`apply_choice`]; the others go straight to
/// the graveyard. `bin` is the card to put there afterwards: the spell's own
/// card, or `None` for a copy.
pub(crate) fn resolve_spell(
    state: &mut State,
    card: Card,
    controller: Player,
    target: Option<Target>,
    bin: Option<Card>,
) -> Result<(), EngineError> {
    match card {
        Card::Unearth => {
            // A target that left the graveyard makes the spell do nothing, and
            // under Grafdigger's Cage the creature can't enter at all.
            let caged = state.controls(Player::You, Card::GrafdiggersCage)
                || state.controls(Player::Opponent, Card::GrafdiggersCage);
            if let Some(Target::Graveyard { card: creature, .. }) = target
                && !caged
                && state
                    .player_mut(controller)
                    .remove_from_graveyard(creature)
                    .is_some()
            {
                state.enter_battlefield(controller, creature, false);
            }
        }
        Card::Counterspell
        | Card::ForceOfWill
        | Card::MentalMisstep
        | Card::SpellSnare
        | Card::Stifle
        | Card::PactOfNegation
        | Card::ConsignToMemory => counter(state, target, false),
        Card::ForceOfNegation => counter(state, target, true),
        Card::Daze | Card::Flusterstorm => return tax(state, controller, bin, target, 1),
        Card::SpellPierce | Card::Miscalculation => return tax(state, controller, bin, target, 2),
        // Counter mode on a spell, destroy mode on a permanent.
        Card::Pyroblast | Card::Hydroblast => match target {
            Some(Target::Permanent { .. }) => remove_permanent(state, card, target, false),
            _ => counter(state, target, false),
        },
        Card::SwordsToPlowshares => remove_permanent(state, card, target, true),
        Card::SnuffOut => remove_permanent(state, card, target, false),
        Card::Silence => state.player_mut(controller.other()).silenced = true,
        Card::VeilOfSummer => {
            // "Draw a card if an opponent has cast a blue or black spell this
            // turn", then the protections for the rest of the turn.
            if state
                .player(controller.other())
                .cast_blue_or_black_this_turn
            {
                state.draw(controller, 1);
            }
            state.player_mut(controller).veiled = true;
        }
        Card::Thoughtseize => {
            state.player_mut(controller).life -= 2;
            return discard_from(state, controller, bin, true);
        }
        Card::Duress => return discard_from(state, controller, bin, false),
        Card::SurgicalExtraction => {
            if let Some(Target::Graveyard { owner, card: named }) = target {
                surgical(state, owner, named);
            }
        }
        Card::MindbreakTrap => {
            // Every spell the other player controls, top down; a spell on
            // top of a spell it targets must go first for the indices.
            let victim = controller.other();
            for index in (0..state.stack.len()).rev() {
                let theirs = matches!(
                    state.stack[index],
                    StackItem::Spell { controller, .. } if controller == victim
                );
                if theirs {
                    let item = crate::engine::remove_stack_item(state, index);
                    if !item.is_copy() {
                        state.player_mut(victim).exile.push(item.card());
                    }
                }
            }
        }
        Card::YawgmothsWill => {
            // Set first so Will itself is exiled below.
            state.player_mut(controller).yawgmoths_will = true;
        }
        Card::FlowState => {
            let k = look_at_top(state, controller, 3);
            if k > 0 {
                let ps = state.player(controller);
                let both =
                    ps.graveyard_has(CardKind::Instant) && ps.graveyard_has(CardKind::Sorcery);
                let take = (if both { 2 } else { 1 }).min(k);
                return pause(state, controller, bin, ChoiceKind::FlowState { take });
            }
        }
        Card::CabalRitual => {
            // Threshold is checked on resolution; Cabal Ritual itself is still
            // on the stack and doesn't count.
            let ps = state.player_mut(controller);
            let amount = if ps.graveyard.len() >= 7 { 5 } else { 3 };
            ps.mana_pool.add_mana(Color::B, amount);
        }
        Card::FranticSearch => {
            state.draw(controller, 2);
            if state.outcome.is_none() {
                return pause(state, controller, bin, ChoiceKind::FranticDiscard);
            }
        }
        Card::Brainstorm => {
            state.draw(controller, 3);
            if state.outcome.is_none() {
                return pause(state, controller, bin, ChoiceKind::BrainstormPutBack);
            }
        }
        Card::Ponder => {
            if look_at_top(state, controller, 3) > 0 {
                return pause(state, controller, bin, ChoiceKind::PonderOrder);
            }
            state.draw(controller, 1);
        }
        Card::Preordain => {
            if look_at_top(state, controller, 2) > 0 {
                return pause(state, controller, bin, ChoiceKind::PreordainScry);
            }
            state.draw(controller, 1);
        }
        Card::Consider => {
            if look_at_top(state, controller, 1) > 0 {
                return pause(state, controller, bin, ChoiceKind::ConsiderSurveil);
            }
            state.draw(controller, 1);
        }
        // The peek at the opponent's hand is ignored: information is open.
        Card::AncestralRecall => state.draw(controller, 3),
        Card::GitaxianProbe => state.draw(controller, 1),
        Card::EdgeOfAutumn => {
            if state.player(controller).land_count() <= 4 {
                return pause(
                    state,
                    controller,
                    bin,
                    ChoiceKind::SearchLand(LandFilter::Basic),
                );
            }
        }
        Card::DarkRitual => state.player_mut(controller).mana_pool.add_mana(Color::B, 3),
        _ => return Err(EngineError::Internal("not an instant or sorcery")),
    }
    if let Some(bin) = bin {
        state.put_in_graveyard(controller, bin);
    }
    Ok(())
}

/// Whether `owner`'s Veil of Summer stops `spell` (cast by the other player)
/// from targeting their permanents: hexproof from blue and from black.
fn hexproof_blocks(state: &State, owner: Player, spell: Card) -> bool {
    state.player(owner).veiled && (spell.is_blue() || spell.is_black())
}

/// Destroys (or exiles) the permanent `target` points at, if it is still
/// there and `spell` may still target it.
fn remove_permanent(state: &mut State, spell: Card, target: Option<Target>, exile: bool) {
    let Some(Target::Permanent { owner, card }) = target else {
        return;
    };
    // A Veil cast in response makes the target illegal: the spell fizzles.
    if hexproof_blocks(state, owner, spell) {
        return;
    }
    let ps = state.player_mut(owner);
    if let Some(idx) = ps.battlefield.iter().position(|p| p.card == card) {
        ps.battlefield.remove(idx);
        if exile {
            ps.exile.push(card);
        } else {
            state.put_in_graveyard(owner, card);
        }
    }
}

/// Thoughtseize, Duress and Grief: the other player discards a nonland card
/// of `controller`'s choice. Nothing to choose means nothing happens.
fn discard_from(
    state: &mut State,
    controller: Player,
    bin: Option<Card>,
    creatures: bool,
) -> Result<(), EngineError> {
    let victim = controller.other();
    let kind = ChoiceKind::DiscardFrom { victim, creatures };
    if discard_candidates(state, kind).is_empty() {
        if let Some(bin) = bin {
            state.put_in_graveyard(controller, bin);
        }
        return Ok(());
    }
    pause(state, controller, bin, kind)
}

/// The distinct cards in the victim's hand a [`ChoiceKind::DiscardFrom`] may
/// pick.
fn discard_candidates(state: &State, kind: ChoiceKind) -> Vec<Card> {
    let ChoiceKind::DiscardFrom { victim, creatures } = kind else {
        return Vec::new();
    };
    let mut cards: Vec<Card> = state
        .player(victim)
        .hand
        .iter()
        .copied()
        .filter(|c| c.kind() != CardKind::Land && (creatures || c.kind() != CardKind::Creature))
        .collect();
    cards.sort();
    cards.dedup();
    cards
}

/// Surgical Extraction on `named` in `owner`'s graveyard: exile every copy
/// from their graveyard, hand and library, then they shuffle. Nothing happens
/// if no copy is in the graveyard any more.
fn surgical(state: &mut State, owner: Player, named: Card) {
    let ps = state.player_mut(owner);
    if !ps.graveyard.contains(&named) {
        return;
    }
    for zone in [&mut ps.graveyard, &mut ps.hand, &mut ps.library] {
        let found = zone.iter().filter(|&&c| c == named).count();
        zone.retain(|&c| c != named);
        ps.exile.extend(std::iter::repeat_n(named, found));
    }
    ps.shuffle();
}

/// How many of the top `n` cards an effect gets to look at. Zero if the
/// library is empty; also records a loss if the library's order is unknown.
fn look_at_top(state: &mut State, player: Player, n: usize) -> usize {
    let ps = state.player(player);
    let k = n.min(ps.library.len());
    if k > 0 && !ps.can_see_top(k) {
        state.lose(LossReason::LibraryUnordered);
        return 0;
    }
    k
}

/// Stops the resolution for a decision by `player`, who also owns `bin`.
fn pause(
    state: &mut State,
    player: Player,
    bin: Option<Card>,
    kind: ChoiceKind,
) -> Result<(), EngineError> {
    pause_for(state, player, player, bin, kind)
}

/// Stops the resolution for a decision by `player` about `bin_owner`'s spell.
fn pause_for(
    state: &mut State,
    player: Player,
    bin_owner: Player,
    bin: Option<Card>,
    kind: ChoiceKind,
) -> Result<(), EngineError> {
    if state.pending.is_some() {
        return Err(EngineError::Internal("two choices pending at once"));
    }
    state.pending = Some(PendingChoice {
        player,
        kind,
        bin,
        bin_owner,
    });
    Ok(())
}

/// Counters the stack item `target` points at, if it is still there: a spell
/// goes to its controller's graveyard (or exile), an ability just vanishes.
fn counter(state: &mut State, target: Option<Target>, exile: bool) {
    let Some(Target::Stack { index, card }) = target else {
        return;
    };
    // The target was fixed up or cleared whenever the stack changed, so a
    // mismatch here is a bug rather than a fizzle; do nothing either way.
    let Some(item) = state.stack.get(index) else {
        return;
    };
    if item.card() != card || cant_be_countered(state, item) {
        return;
    }
    let item = crate::engine::remove_stack_item(state, index);
    // A countered copy just ceases to exist.
    if !item.is_copy()
        && let StackItem::Spell {
            card, controller, ..
        } = item
    {
        if exile {
            state.player_mut(controller).exile.push(card);
        } else {
            state.put_in_graveyard(controller, card);
        }
    }
}

/// Veil of Summer: "spells you control can't be countered this turn".
fn cant_be_countered(state: &State, item: &StackItem) -> bool {
    matches!(item, StackItem::Spell { controller, .. } if state.player(*controller).veiled)
}

/// "Counter target spell unless its controller pays {amount}": the target's
/// controller decides, and `bin` is binned to `controller`'s graveyard once
/// they have.
fn tax(
    state: &mut State,
    controller: Player,
    bin: Option<Card>,
    target: Option<Target>,
    amount: u8,
) -> Result<(), EngineError> {
    if let Some(target @ Target::Stack { index, card: hit }) = target
        && let Some(item) = state.stack.get(index)
        && item.card() == hit
        && !cant_be_countered(state, item)
    {
        let payer = item.controller();
        return pause_for(
            state,
            payer,
            controller,
            bin,
            ChoiceKind::PayTax { amount, target },
        );
    }
    // The target is gone: the spell does nothing.
    if let Some(bin) = bin {
        state.put_in_graveyard(controller, bin);
    }
    Ok(())
}

/// Puts `count` copies of `original` on the stack, each aimed at the matching
/// entry of `targets` (or at nothing, if `targets` is empty).
fn push_copies(state: &mut State, original: StackItem, targets: &[Target], count: u8) {
    let StackItem::Spell {
        card, controller, ..
    } = original
    else {
        return;
    };
    for i in 0..usize::from(count) {
        state.stack.push(StackItem::Spell {
            card,
            controller,
            target: targets.get(i).copied(),
            mode: SpellMode::Copy,
        });
    }
}

pub(crate) fn resolve_trigger(
    state: &mut State,
    kind: TriggerKind,
    controller: Player,
) -> Result<(), EngineError> {
    match kind {
        TriggerKind::OracleEnters => {
            let x = usize::from(state.player(controller).blue_devotion());
            let ps = state.player_mut(controller);
            if x >= ps.library.len() {
                state.win(controller);
            } else if x > 0 {
                // The looked-at cards go to the bottom in a random order.
                ps.library_ordered = false;
            }
        }
        TriggerKind::Surveil => {
            if look_at_top(state, controller, 1) > 0 {
                return pause(state, controller, None, ChoiceKind::Surveil);
            }
        }
        TriggerKind::Copies { count } => {
            // The original is the item directly below this trigger: anything
            // cast after the trigger has already resolved.
            let Some(&original) = state.stack.last() else {
                return Ok(());
            };
            let targets = original
                .card()
                .target_options(state, controller)
                .unwrap_or_default();
            match targets.len() {
                // No legal target: the copies would fizzle, so skip them.
                0 => {}
                // One legal target: no decision to make.
                1 => push_copies(
                    state,
                    original,
                    &vec![targets[0]; usize::from(count)],
                    count,
                ),
                _ => return pause(state, controller, None, ChoiceKind::CopyTargets { count }),
            }
        }
        TriggerKind::GriefEnters => return discard_from(state, controller, None, true),
        TriggerKind::BowmastersPing => {
            return pause(state, controller, None, ChoiceKind::PingTarget);
        }
    }
    Ok(())
}

/// Resolves an activated ability whose costs were paid on activation.
pub(crate) fn resolve_ability(
    state: &mut State,
    kind: AbilityKind,
    source: Card,
    controller: Player,
) -> Result<(), EngineError> {
    match kind {
        AbilityKind::Cycling => state.draw(controller, 1),
        AbilityKind::Fetch => {
            let types = source
                .fetch_filter()
                .ok_or(EngineError::Internal("fetch ability on a non-fetchland"))?;
            return pause(
                state,
                controller,
                None,
                ChoiceKind::SearchLand(LandFilter::Types(types)),
            );
        }
        AbilityKind::DoomDraw => {
            state.draw(controller, 1);
            let ps = state.player_mut(controller);
            ps.life -= 1;
            if ps.library.is_empty() {
                state.win(controller);
            }
        }
        AbilityKind::JacePlusOne { target } => {
            // Milling an unordered library is as order-dependent as drawing.
            let k = look_at_top(state, target, 2);
            if state.outcome.is_some() {
                return Ok(());
            }
            state.mill(target, k);
            state.draw(controller, 1);
        }
        AbilityKind::TeferiPlusOne => {
            state.player_mut(controller).sorceries_have_flash = true;
        }
        AbilityKind::TeferiMinusThree { target } => {
            if let Some(Target::Permanent { owner, card }) = target {
                let ps = state.player_mut(owner);
                if let Some(idx) = ps.battlefield.iter().position(|p| p.card == card) {
                    ps.battlefield.remove(idx);
                    ps.hand.push(card);
                }
            }
            state.draw(controller, 1);
        }
    }
    Ok(())
}

/// Every legal answer to a pending choice.
pub(crate) fn choice_options(state: &State, pending: &PendingChoice) -> Vec<Choice> {
    let ps = state.player(pending.player);
    let mut out = Vec::new();
    match pending.kind {
        ChoiceKind::BrainstormPutBack => {
            for pair in ordered_selections(&ps.hand, 2) {
                out.push(Choice::BrainstormPutBack([pair[0], pair[1]]));
            }
        }
        ChoiceKind::PonderOrder => {
            let k = ps.library.len().min(3);
            for order in ordered_selections(&ps.library[..k], k) {
                out.push(Choice::PonderOrder(order));
            }
        }
        ChoiceKind::PreordainScry => {
            let k = ps.library.len().min(2);
            let looked = &ps.library[..k];
            for mask in 0..(1u32 << k) {
                let mut top_set = Vec::new();
                let mut bottom_set = Vec::new();
                for (i, &card) in looked.iter().enumerate() {
                    if mask & (1 << i) != 0 {
                        top_set.push(card);
                    } else {
                        bottom_set.push(card);
                    }
                }
                for top in ordered_selections(&top_set, top_set.len()) {
                    for bottom in ordered_selections(&bottom_set, bottom_set.len()) {
                        let choice = Choice::PreordainScry {
                            top: top.clone(),
                            bottom,
                        };
                        if !out.contains(&choice) {
                            out.push(choice);
                        }
                    }
                }
            }
        }
        ChoiceKind::ConsiderSurveil | ChoiceKind::Surveil => {
            out.push(Choice::Surveil {
                to_graveyard: false,
            });
            out.push(Choice::Surveil { to_graveyard: true });
        }
        ChoiceKind::SearchLand(filter) => {
            let mut found: Vec<Card> = ps
                .library
                .iter()
                .copied()
                .filter(|&c| filter.matches(c))
                .collect();
            found.sort();
            found.dedup();
            out.extend(found.into_iter().map(|c| Choice::SearchLand(Some(c))));
            out.push(Choice::SearchLand(None));
        }
        ChoiceKind::FlowState { take } => {
            let k = ps.library.len().min(3);
            let looked = &ps.library[..k];
            for to_hand in unordered_selections(looked, take) {
                let mut rest = looked.to_vec();
                for card in &to_hand {
                    if let Some(pos) = rest.iter().position(|c| c == card) {
                        rest.remove(pos);
                    }
                }
                for bottom in ordered_selections(&rest, rest.len()) {
                    out.push(Choice::FlowState {
                        to_hand: to_hand.clone(),
                        bottom,
                    });
                }
            }
        }
        ChoiceKind::FranticDiscard => {
            let k = ps.hand.len().min(2);
            out.extend(
                unordered_selections(&ps.hand, k)
                    .into_iter()
                    .map(Choice::Discard),
            );
        }
        ChoiceKind::UntapLands => {
            let tapped = tapped_lands(ps);
            let k = tapped.len().min(3);
            out.extend(
                unordered_selections(&tapped, k)
                    .into_iter()
                    .map(Choice::UntapLands),
            );
        }
        ChoiceKind::PayTax { amount, .. } => {
            // You pay from mana already in the pool (tap in response, not
            // now); the opponent may tap lands on the spot.
            out.extend(
                tax_payments(state, pending.player, amount)
                    .into_iter()
                    .map(Choice::PayTax),
            );
            out.push(Choice::DeclineTax);
        }
        ChoiceKind::CopyTargets { count } => {
            if let Some(original) = state.stack.last() {
                let targets = original
                    .card()
                    .target_options(state, pending.player)
                    .unwrap_or_default();
                out.extend(
                    multisets(&targets, usize::from(count))
                        .into_iter()
                        .map(Choice::CopyTargets),
                );
            }
        }
        kind @ ChoiceKind::DiscardFrom { .. } => {
            out.extend(
                discard_candidates(state, kind)
                    .into_iter()
                    .map(|c| Choice::Discard(vec![c])),
            );
        }
        ChoiceKind::PingTarget => {
            out.extend(
                ping_targets(state, pending.player)
                    .into_iter()
                    .map(Choice::Ping),
            );
        }
    }
    out
}

/// The tapped lands `ps` controls, one entry per permanent.
fn tapped_lands(ps: &PlayerState) -> Vec<Card> {
    ps.battlefield
        .iter()
        .filter(|p| p.tapped && p.card.kind() == CardKind::Land)
        .map(|p| p.card)
        .collect()
}

/// Finishes a paused resolution with the player's answer.
pub(crate) fn apply_choice(
    state: &mut State,
    pending: PendingChoice,
    choice: &Choice,
) -> Result<(), EngineError> {
    let player = pending.player;
    let illegal = || EngineError::IllegalAction(crate::engine::Action::Choose(choice.clone()));
    match (pending.kind, choice) {
        (ChoiceKind::BrainstormPutBack, Choice::BrainstormPutBack([top, second])) => {
            let ps = state.player_mut(player);
            ps.remove_from_hand(*top).ok_or_else(illegal)?;
            ps.remove_from_hand(*second).ok_or_else(illegal)?;
            ps.library.insert(0, *second);
            ps.library.insert(0, *top);
        }
        (ChoiceKind::PonderOrder, Choice::PonderOrder(order)) => {
            let ps = state.player_mut(player);
            let k = order.len();
            if k != ps.library.len().min(3) || !same_cards(order, &ps.library[..k]) {
                return Err(illegal());
            }
            ps.library[..k].copy_from_slice(order);
            if k >= ps.library.len() {
                ps.library_ordered = true;
            }
            state.draw(player, 1);
        }
        (ChoiceKind::PreordainScry, Choice::PreordainScry { top, bottom }) => {
            let ps = state.player_mut(player);
            let k = top.len() + bottom.len();
            let mut chosen = top.clone();
            chosen.extend(bottom);
            if k != ps.library.len().min(2) || !same_cards(&chosen, &ps.library[..k]) {
                return Err(illegal());
            }
            let rest = ps.library.split_off(k);
            if rest.is_empty() {
                // Every card was looked at and placed, so the order is known again.
                ps.library_ordered = true;
            }
            ps.library = top.clone();
            ps.library.extend(rest);
            ps.library.extend(bottom);
            state.draw(player, 1);
        }
        (ChoiceKind::ConsiderSurveil | ChoiceKind::Surveil, Choice::Surveil { to_graveyard }) => {
            let ps = state.player_mut(player);
            if ps.library.is_empty() {
                return Err(illegal());
            }
            if *to_graveyard {
                state.mill(player, 1);
            }
            if pending.kind == ChoiceKind::ConsiderSurveil {
                state.draw(player, 1);
            }
        }
        (ChoiceKind::SearchLand(filter), Choice::SearchLand(found)) => {
            let ps = state.player_mut(player);
            if let Some(card) = found {
                if !filter.matches(*card) {
                    return Err(illegal());
                }
                let pos = ps
                    .library
                    .iter()
                    .position(|c| c == card)
                    .ok_or_else(illegal)?;
                ps.library.remove(pos);
            }
            ps.shuffle();
            if let Some(card) = found {
                // Edge of Autumn's basic enters tapped; fetched lands enter as
                // they normally would.
                let tapped = filter == LandFilter::Basic || card.enters_tapped();
                state.enter_battlefield(player, *card, tapped);
            }
        }
        (ChoiceKind::FlowState { take }, Choice::FlowState { to_hand, bottom }) => {
            let ps = state.player_mut(player);
            let k = ps.library.len().min(3);
            let mut chosen = to_hand.clone();
            chosen.extend(bottom);
            if to_hand.len() != take || chosen.len() != k || !same_cards(&chosen, &ps.library[..k])
            {
                return Err(illegal());
            }
            let rest = ps.library.split_off(k);
            if rest.is_empty() {
                // Every card was looked at and placed, so the order is known again.
                ps.library_ordered = true;
            }
            // Putting a card into hand this way is not a draw.
            ps.hand.extend(to_hand);
            ps.library = rest;
            ps.library.extend(bottom);
        }
        (ChoiceKind::FranticDiscard, Choice::Discard(cards)) => {
            let ps = state.player_mut(player);
            if cards.len() != ps.hand.len().min(2) {
                return Err(illegal());
            }
            for &card in cards {
                state
                    .player_mut(player)
                    .remove_from_hand(card)
                    .ok_or_else(illegal)?;
                state.put_in_graveyard(player, card);
            }
            if !tapped_lands(state.player(player)).is_empty() {
                // The untap step is a second decision; the spell is binned
                // once that one is answered.
                return pause(state, player, pending.bin, ChoiceKind::UntapLands);
            }
        }
        (ChoiceKind::UntapLands, Choice::UntapLands(cards)) => {
            let ps = state.player_mut(player);
            if cards.len() != tapped_lands(ps).len().min(3) {
                return Err(illegal());
            }
            for &card in cards {
                let idx = ps
                    .battlefield
                    .iter()
                    .position(|p| p.card == card && p.tapped && card.kind() == CardKind::Land)
                    .ok_or_else(illegal)?;
                ps.battlefield[idx].tapped = false;
            }
        }
        (ChoiceKind::PayTax { amount, .. }, Choice::PayTax(mana)) => {
            if !tax_payments(state, player, amount).contains(mana) {
                return Err(illegal());
            }
            pay_mana(state.player_mut(player), mana).ok_or_else(illegal)?;
        }
        (ChoiceKind::PayTax { target, .. }, Choice::DeclineTax) => {
            counter(state, Some(target), false);
        }
        (ChoiceKind::CopyTargets { count }, Choice::CopyTargets(targets)) => {
            let original = *state.stack.last().ok_or_else(illegal)?;
            let legal = original
                .card()
                .target_options(state, player)
                .unwrap_or_default();
            if !multisets(&legal, usize::from(count)).contains(targets) {
                return Err(illegal());
            }
            push_copies(state, original, targets, count);
        }
        (kind @ ChoiceKind::DiscardFrom { victim, .. }, Choice::Discard(cards)) => {
            let [card] = cards.as_slice() else {
                return Err(illegal());
            };
            if !discard_candidates(state, kind).contains(card) {
                return Err(illegal());
            }
            state
                .player_mut(victim)
                .remove_from_hand(*card)
                .ok_or_else(illegal)?;
            state.put_in_graveyard(victim, *card);
        }
        (ChoiceKind::PingTarget, Choice::Ping(target)) => {
            if !ping_targets(state, player).contains(target) {
                return Err(illegal());
            }
            match *target {
                Target::Player(victim) => state.player_mut(victim).life -= 1,
                Target::Permanent { owner, card } => {
                    let ps = state.player_mut(owner);
                    let idx = ps
                        .battlefield
                        .iter()
                        .position(|p| p.card == card)
                        .ok_or_else(illegal)?;
                    ps.battlefield[idx].damage += 1;
                }
                _ => return Err(illegal()),
            }
        }
        _ => return Err(illegal()),
    }
    if let Some(card) = pending.bin {
        state.put_in_graveyard(pending.bin_owner, card);
    }
    Ok(())
}

/// Where Orcish Bowmasters' controller may send the ping: the other player,
/// or one of their creatures.
fn ping_targets(state: &State, controller: Player) -> Vec<Target> {
    let victim = controller.other();
    let mut out = vec![Target::Player(victim)];
    let mut creatures: Vec<Card> = state
        .player(victim)
        .battlefield
        .iter()
        .map(|p| p.card)
        .filter(|c| c.kind() == CardKind::Creature)
        .collect();
    creatures.sort();
    creatures.dedup();
    out.extend(creatures.into_iter().map(|card| Target::Permanent {
        owner: victim,
        card,
    }));
    out
}

/// Same multiset of cards, order ignored.
fn same_cards(a: &[Card], b: &[Card]) -> bool {
    let mut a = a.to_vec();
    let mut b = b.to_vec();
    a.sort();
    b.sort();
    a == b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Action, apply, legal_actions};
    use crate::mana::ManaPool;
    use crate::state::test_support::main_phase;
    use crate::state::{Outcome, Permanent, Zone};

    /// Resolves the top of the stack with two passes.
    fn pass_twice(state: &State) -> State {
        apply(&apply(state, &Action::Pass).unwrap(), &Action::Pass).unwrap()
    }

    /// Puts `card` on the stack as your spell and resolves it with two passes.
    fn resolve(state: State, card: Card) -> State {
        resolve_targeting(state, card, None)
    }

    fn resolve_targeting(mut state: State, card: Card, target: Option<Target>) -> State {
        state.stack.push(StackItem::Spell {
            card,
            controller: Player::You,
            target,
            mode: SpellMode::Normal,
        });
        pass_twice(&state)
    }

    fn with_trigger(mut state: State, source: Card, kind: TriggerKind) -> State {
        state.stack.push(StackItem::Trigger {
            source,
            kind,
            controller: Player::You,
        });
        state
    }

    fn with_ability(mut state: State, source: Card, kind: AbilityKind) -> State {
        state.stack.push(StackItem::Ability {
            source,
            kind,
            controller: Player::You,
        });
        state
    }

    #[test]
    fn card_data() {
        assert_eq!(Card::ThassasOracle.blue_devotion(), 2);
        assert_eq!(Card::JaceWielderOfMysteries.blue_devotion(), 3);
        assert_eq!(Card::LaboratoryManiac.blue_devotion(), 1);
        assert_eq!(Card::DoctorDoom.blue_devotion(), 0);
        assert_eq!(Card::MoxSapphire.blue_devotion(), 0);
        assert_eq!(Card::ThassasOracle.kind(), CardKind::Creature);
        assert_eq!(Card::JaceWielderOfMysteries.kind(), CardKind::Planeswalker);
        assert_eq!(Card::Brainstorm.kind(), CardKind::Instant);
        assert_eq!(Card::Ponder.kind(), CardKind::Sorcery);
        assert_eq!(Card::LionsEyeDiamond.kind(), CardKind::Artifact);
        assert_eq!(Card::UndergroundSea.kind(), CardKind::Land);
        assert_eq!(Card::Doomsday.mana_cost().mana_value(), 3);
        assert_eq!(Card::GitaxianProbe.mana_cost().mana_value(), 1);
        assert_eq!(Card::JaceWielderOfMysteries.mana_cost().mana_value(), 4);
        assert_eq!(Card::LionsEyeDiamond.mana_cost(), ManaCost::FREE);
        assert!(!Card::StreetWraith.castable());
        assert!(Card::DoctorDoom.castable());
        assert_eq!(Card::JaceWielderOfMysteries.starting_loyalty(), 4);
        assert_eq!(Card::ThassasOracle.starting_loyalty(), 0);
    }

    #[test]
    fn land_data() {
        assert_eq!(Card::UndergroundSea.mana_colors(), vec![Color::U, Color::B]);
        assert_eq!(Card::Taiga.mana_colors(), vec![Color::R, Color::G]);
        assert_eq!(Card::Island.mana_colors(), vec![Color::U]);
        assert_eq!(Card::MoxJet.mana_colors(), vec![Color::B]);
        assert_eq!(Card::ThassasOracle.mana_ability(), None);
        assert_eq!(Card::PollutedDelta.mana_ability(), None);
        assert_eq!(Card::UndercitySewers.mana_ability(), Some(ManaAbility::Tap));
        assert!(Card::UndercitySewers.enters_tapped());
        assert!(!Card::UndergroundSea.enters_tapped());
        assert!(Card::Island.is_basic_land());
        assert!(!Card::UndergroundSea.is_basic_land());
        let delta = LandFilter::Types(Card::PollutedDelta.fetch_filter().unwrap());
        assert!(delta.matches(Card::Island));
        assert!(delta.matches(Card::Swamp));
        assert!(delta.matches(Card::UndergroundSea));
        assert!(delta.matches(Card::Tundra));
        assert!(delta.matches(Card::UndercitySewers));
        assert!(!delta.matches(Card::Plains));
        assert!(!delta.matches(Card::Taiga));
        assert!(!delta.matches(Card::FloodedStrand));
        let strand = LandFilter::Types(Card::FloodedStrand.fetch_filter().unwrap());
        assert!(strand.matches(Card::Plains));
        assert!(strand.matches(Card::MeticulousArchive));
        assert!(!strand.matches(Card::Swamp));
        assert!(LandFilter::Basic.matches(Card::Forest));
        assert!(!LandFilter::Basic.matches(Card::Bayou));
    }

    #[test]
    fn ancestral_draws_three() {
        let state = main_phase(PlayerState {
            library: vec![Card::Island, Card::Swamp, Card::Plains, Card::Forest],
            ..PlayerState::default()
        });
        let state = resolve(state, Card::AncestralRecall);
        assert_eq!(
            state.you.hand,
            vec![Card::Plains, Card::Island, Card::Swamp]
        );
        assert_eq!(state.you.library, vec![Card::Forest]);
        assert_eq!(state.you.graveyard, vec![Card::AncestralRecall]);
        assert_eq!(state.you.cards_drawn_this_turn, 3);
    }

    #[test]
    fn probe_draws_one() {
        let state = main_phase(PlayerState {
            library: vec![Card::Island, Card::Swamp],
            ..PlayerState::default()
        });
        let state = resolve(state, Card::GitaxianProbe);
        assert_eq!(state.you.hand, vec![Card::Island]);
        assert_eq!(state.you.graveyard, vec![Card::GitaxianProbe]);
    }

    #[test]
    fn dark_ritual_adds_bbb() {
        let state = resolve(main_phase(PlayerState::default()), Card::DarkRitual);
        assert_eq!(state.you.mana_pool, ManaPool::of(Color::B, 3));
        assert_eq!(state.you.graveyard, vec![Card::DarkRitual]);
    }

    #[test]
    fn brainstorm_draws_three_then_puts_back_any_two() {
        let state = main_phase(PlayerState {
            hand: vec![Card::Island],
            library: vec![Card::ThassasOracle, Card::Swamp, Card::Plains, Card::Forest],
            ..PlayerState::default()
        });
        let state = resolve(state, Card::Brainstorm);
        assert_eq!(state.you.hand.len(), 4);
        let pending = state.pending.unwrap();
        assert_eq!(pending.kind, ChoiceKind::BrainstormPutBack);
        // Nothing but choices while a decision is pending.
        let actions = legal_actions(&state);
        assert!(actions.iter().all(|a| matches!(a, Action::Choose(_))));
        assert_eq!(actions.len(), 12); // ordered pairs of 4 distinct cards

        // Put back a card that was already in hand, not just a drawn one.
        let next = apply(
            &state,
            &Action::Choose(Choice::BrainstormPutBack([Card::Island, Card::Swamp])),
        )
        .unwrap();
        assert_eq!(
            next.you.library,
            vec![Card::Island, Card::Swamp, Card::Forest]
        );
        assert_eq!(next.you.hand, vec![Card::ThassasOracle, Card::Plains]);
        assert_eq!(next.you.graveyard, vec![Card::Brainstorm]);
        assert_eq!(next.pending, None);
        assert_eq!(next.priority, Player::You);
    }

    #[test]
    fn ponder_reorders_top_three_without_shuffling_then_draws() {
        let state = main_phase(PlayerState {
            library: vec![Card::Island, Card::Swamp, Card::ThassasOracle, Card::Forest],
            ..PlayerState::default()
        });
        let state = resolve(state, Card::Ponder);
        assert_eq!(legal_actions(&state).len(), 6);
        let next = apply(
            &state,
            &Action::Choose(Choice::PonderOrder(vec![
                Card::ThassasOracle,
                Card::Island,
                Card::Swamp,
            ])),
        )
        .unwrap();
        assert_eq!(next.you.hand, vec![Card::ThassasOracle]);
        assert_eq!(
            next.you.library,
            vec![Card::Island, Card::Swamp, Card::Forest]
        );
        assert!(next.you.library_ordered);
        assert_eq!(next.you.graveyard, vec![Card::Ponder]);
    }

    #[test]
    fn ponder_on_fully_unordered_library_restores_order() {
        let state = main_phase(PlayerState {
            library: vec![Card::Island, Card::Swamp, Card::ThassasOracle],
            library_ordered: false,
            ..PlayerState::default()
        });
        let state = resolve(state, Card::Ponder);
        assert_eq!(state.outcome, None);
        let next = apply(
            &state,
            &Action::Choose(Choice::PonderOrder(vec![
                Card::ThassasOracle,
                Card::Island,
                Card::Swamp,
            ])),
        )
        .unwrap();
        assert!(next.you.library_ordered);
        assert_eq!(next.you.hand, vec![Card::ThassasOracle]);
    }

    #[test]
    fn ponder_on_partly_unordered_library_fails() {
        let state = main_phase(PlayerState {
            library: vec![Card::Island, Card::Swamp, Card::ThassasOracle, Card::Forest],
            library_ordered: false,
            ..PlayerState::default()
        });
        let state = resolve(state, Card::Ponder);
        assert_eq!(
            state.outcome,
            Some(Outcome::Loss(LossReason::LibraryUnordered))
        );
    }

    #[test]
    fn preordain_scries_two_then_draws() {
        let state = main_phase(PlayerState {
            library: vec![Card::Island, Card::ThassasOracle, Card::Forest],
            ..PlayerState::default()
        });
        let state = resolve(state, Card::Preordain);
        // {} / {a} / {b} / {a,b}: 2 + 1 + 1 + 2 orderings.
        assert_eq!(legal_actions(&state).len(), 6);
        let next = apply(
            &state,
            &Action::Choose(Choice::PreordainScry {
                top: vec![Card::ThassasOracle],
                bottom: vec![Card::Island],
            }),
        )
        .unwrap();
        assert_eq!(next.you.hand, vec![Card::ThassasOracle]);
        assert_eq!(next.you.library, vec![Card::Forest, Card::Island]);
        assert_eq!(next.you.graveyard, vec![Card::Preordain]);
    }

    #[test]
    fn consider_surveils_then_draws() {
        let state = main_phase(PlayerState {
            library: vec![Card::Island, Card::ThassasOracle],
            ..PlayerState::default()
        });
        let state = resolve(state, Card::Consider);
        assert_eq!(legal_actions(&state).len(), 2);
        let binned = apply(
            &state,
            &Action::Choose(Choice::Surveil { to_graveyard: true }),
        )
        .unwrap();
        assert_eq!(binned.you.hand, vec![Card::ThassasOracle]);
        assert!(binned.you.library.is_empty());
        assert_eq!(binned.you.graveyard, vec![Card::Consider, Card::Island]);
        let kept = apply(
            &state,
            &Action::Choose(Choice::Surveil {
                to_graveyard: false,
            }),
        )
        .unwrap();
        assert_eq!(kept.you.hand, vec![Card::Island]);
    }

    #[test]
    fn cantrips_on_an_empty_library_lose() {
        for card in [
            Card::Ponder,
            Card::Preordain,
            Card::Consider,
            Card::GitaxianProbe,
        ] {
            let state = resolve(main_phase(PlayerState::default()), card);
            assert_eq!(
                state.outcome,
                Some(Outcome::Loss(LossReason::DrewFromEmptyLibrary)),
                "{card}"
            );
        }
    }

    #[test]
    fn surveil_trigger_offers_keep_or_bin_without_drawing() {
        let state = main_phase(PlayerState {
            library: vec![Card::Island, Card::Swamp],
            ..PlayerState::default()
        });
        let state = pass_twice(&with_trigger(
            state,
            Card::UndercitySewers,
            TriggerKind::Surveil,
        ));
        assert_eq!(state.pending.unwrap().kind, ChoiceKind::Surveil);
        assert_eq!(legal_actions(&state).len(), 2);
        let binned = apply(
            &state,
            &Action::Choose(Choice::Surveil { to_graveyard: true }),
        )
        .unwrap();
        assert_eq!(binned.you.library, vec![Card::Swamp]);
        assert_eq!(binned.you.graveyard, vec![Card::Island]);
        assert!(binned.you.hand.is_empty());
        let kept = apply(
            &state,
            &Action::Choose(Choice::Surveil {
                to_graveyard: false,
            }),
        )
        .unwrap();
        assert_eq!(kept.you.library, vec![Card::Island, Card::Swamp]);
    }

    #[test]
    fn surveil_trigger_on_an_empty_library_does_nothing() {
        let state = pass_twice(&with_trigger(
            main_phase(PlayerState::default()),
            Card::UndercitySewers,
            TriggerKind::Surveil,
        ));
        assert_eq!(state.pending, None);
        assert_eq!(state.outcome, None);
    }

    #[test]
    fn edge_of_autumn_cast_fetches_a_basic_tapped_and_shuffles() {
        let state = main_phase(PlayerState {
            battlefield: vec![
                Permanent::tapped(Card::TropicalIsland),
                Permanent::tapped(Card::Island),
            ],
            library: vec![Card::Swamp, Card::Bayou, Card::Island, Card::Swamp],
            ..PlayerState::default()
        });
        let state = resolve(state, Card::EdgeOfAutumn);
        assert_eq!(
            state.pending.unwrap().kind,
            ChoiceKind::SearchLand(LandFilter::Basic)
        );
        let options = legal_actions(&state);
        // Island, Swamp (once, despite two copies), or nothing. Bayou is not basic.
        assert_eq!(options.len(), 3);
        assert!(options.contains(&Action::Choose(Choice::SearchLand(None))));
        assert!(!options.contains(&Action::Choose(Choice::SearchLand(Some(Card::Bayou)))));

        let next = apply(
            &state,
            &Action::Choose(Choice::SearchLand(Some(Card::Swamp))),
        )
        .unwrap();
        assert!(
            next.you
                .battlefield
                .contains(&Permanent::tapped(Card::Swamp))
        );
        assert_eq!(next.you.library.len(), 3);
        assert!(!next.you.library_ordered);
        assert_eq!(next.you.graveyard, vec![Card::EdgeOfAutumn]);
        assert_eq!(next.pending, None);

        let nothing = apply(&state, &Action::Choose(Choice::SearchLand(None))).unwrap();
        assert_eq!(nothing.you.library.len(), 4);
        assert!(!nothing.you.library_ordered, "a shuffle still happened");
    }

    #[test]
    fn edge_of_autumn_search_with_one_card_left_keeps_the_order_known() {
        let state = main_phase(PlayerState {
            library: vec![Card::Swamp, Card::Island],
            ..PlayerState::default()
        });
        let state = resolve(state, Card::EdgeOfAutumn);
        let next = apply(
            &state,
            &Action::Choose(Choice::SearchLand(Some(Card::Swamp))),
        )
        .unwrap();
        assert_eq!(next.you.library, vec![Card::Island]);
        assert!(next.you.library_ordered);
    }

    #[test]
    fn edge_of_autumn_does_nothing_with_five_lands() {
        let lands = [
            Card::Island,
            Card::Island,
            Card::Swamp,
            Card::Forest,
            Card::PollutedDelta,
        ];
        let state = main_phase(PlayerState {
            battlefield: lands.iter().map(|&c| Permanent::tapped(c)).collect(),
            library: vec![Card::Swamp],
            ..PlayerState::default()
        });
        let state = resolve(state, Card::EdgeOfAutumn);
        assert_eq!(state.pending, None);
        assert_eq!(state.you.library, vec![Card::Swamp]);
        assert!(state.you.library_ordered);
        assert_eq!(state.you.graveyard, vec![Card::EdgeOfAutumn]);
    }

    #[test]
    fn cycling_ability_draws_a_card() {
        let state = main_phase(PlayerState {
            library: vec![Card::Island],
            ..PlayerState::default()
        });
        let state = pass_twice(&with_ability(
            state,
            Card::StreetWraith,
            AbilityKind::Cycling,
        ));
        assert_eq!(state.you.hand, vec![Card::Island]);
    }

    #[test]
    fn fetch_ability_offers_matching_lands_and_shuffles() {
        let state = main_phase(PlayerState {
            library: vec![
                Card::Island,
                Card::Swamp,
                Card::UndergroundSea,
                Card::Plains,
                Card::Tundra,
                Card::UndercitySewers,
                Card::Taiga,
            ],
            ..PlayerState::default()
        });
        let delta = pass_twice(&with_ability(
            state.clone(),
            Card::PollutedDelta,
            AbilityKind::Fetch,
        ));
        let delta_options = legal_actions(&delta);
        // Island, Swamp, Sea, Tundra, Sewers, nothing.
        assert_eq!(delta_options.len(), 6);
        assert!(!delta_options.contains(&Action::Choose(Choice::SearchLand(Some(Card::Plains)))));

        let strand = pass_twice(&with_ability(
            state,
            Card::FloodedStrand,
            AbilityKind::Fetch,
        ));
        let strand_options = legal_actions(&strand);
        // Island, Sea, Plains, Tundra, Sewers, nothing.
        assert_eq!(strand_options.len(), 6);
        assert!(!strand_options.contains(&Action::Choose(Choice::SearchLand(Some(Card::Swamp)))));

        // A fetched dual enters untapped; a fetched Sewers enters tapped and
        // its surveil trigger goes on the stack.
        let sea = apply(
            &delta,
            &Action::Choose(Choice::SearchLand(Some(Card::UndergroundSea))),
        )
        .unwrap();
        assert!(
            sea.you
                .battlefield
                .contains(&Permanent::untapped(Card::UndergroundSea))
        );
        assert_eq!(sea.you.library.len(), 6);
        assert!(!sea.you.library_ordered);
        assert!(sea.stack.is_empty());

        let sewers = apply(
            &delta,
            &Action::Choose(Choice::SearchLand(Some(Card::UndercitySewers))),
        )
        .unwrap();
        assert!(
            sewers
                .you
                .battlefield
                .contains(&Permanent::tapped(Card::UndercitySewers))
        );
        assert!(matches!(
            sewers.stack.as_slice(),
            [StackItem::Trigger {
                kind: TriggerKind::Surveil,
                ..
            }]
        ));
    }

    #[test]
    fn doom_ability_draws_loses_life_and_wins_on_an_empty_library() {
        let state = main_phase(PlayerState {
            battlefield: vec![Permanent::tapped(Card::DoctorDoom)],
            library: vec![Card::Island],
            life: 1,
            ..PlayerState::default()
        });
        let state = pass_twice(&with_ability(
            state,
            Card::DoctorDoom,
            AbilityKind::DoomDraw,
        ));
        assert_eq!(state.you.hand, vec![Card::Island]);
        assert_eq!(state.you.life, 0);
        assert_eq!(state.outcome, Some(Outcome::Win), "wins even at 0 life");
    }

    #[test]
    fn doom_ability_wins_even_without_drawing_a_card() {
        let state = main_phase(PlayerState {
            battlefield: vec![Permanent::tapped(Card::DoctorDoom)],
            ..PlayerState::default()
        });
        let state = pass_twice(&with_ability(
            state,
            Card::DoctorDoom,
            AbilityKind::DoomDraw,
        ));
        assert_eq!(state.outcome, Some(Outcome::Win));
    }

    #[test]
    fn doom_ability_with_two_cards_left_does_not_win() {
        let state = main_phase(PlayerState {
            battlefield: vec![Permanent::tapped(Card::DoctorDoom)],
            library: vec![Card::Island, Card::Swamp],
            life: 5,
            ..PlayerState::default()
        });
        let state = pass_twice(&with_ability(
            state,
            Card::DoctorDoom,
            AbilityKind::DoomDraw,
        ));
        assert_eq!(state.outcome, None);
        assert_eq!(state.you.library, vec![Card::Swamp]);
        assert_eq!(state.you.life, 4);
    }

    #[test]
    fn jace_plus_one_mills_the_target_then_draws() {
        let state = main_phase(PlayerState {
            battlefield: vec![Permanent::untapped(Card::JaceWielderOfMysteries)],
            library: vec![Card::Island, Card::Swamp, Card::Plains, Card::Forest],
            ..PlayerState::default()
        });
        let you = pass_twice(&with_ability(
            state.clone(),
            Card::JaceWielderOfMysteries,
            AbilityKind::JacePlusOne {
                target: Player::You,
            },
        ));
        assert_eq!(you.you.graveyard, vec![Card::Island, Card::Swamp]);
        assert_eq!(you.you.hand, vec![Card::Plains]);
        assert_eq!(you.you.library, vec![Card::Forest]);

        let them = pass_twice(&with_ability(
            state,
            Card::JaceWielderOfMysteries,
            AbilityKind::JacePlusOne {
                target: Player::Opponent,
            },
        ));
        assert!(them.you.graveyard.is_empty());
        assert_eq!(them.you.hand, vec![Card::Island]);
        assert_eq!(them.you.library.len(), 3);
    }

    #[test]
    fn jace_plus_one_on_two_cards_wins_through_his_static() {
        let state = main_phase(PlayerState {
            battlefield: vec![Permanent::untapped(Card::JaceWielderOfMysteries)],
            library: vec![Card::Island, Card::Swamp],
            ..PlayerState::default()
        });
        let state = pass_twice(&with_ability(
            state,
            Card::JaceWielderOfMysteries,
            AbilityKind::JacePlusOne {
                target: Player::You,
            },
        ));
        assert_eq!(state.outcome, Some(Outcome::Win));
    }

    #[test]
    fn oracle_wins_when_devotion_covers_library() {
        let state = main_phase(PlayerState {
            library: vec![Card::Island, Card::Swamp],
            mana_pool: ManaPool::of(Color::U, 2),
            hand: vec![Card::ThassasOracle],
            ..PlayerState::default()
        });
        let cast = apply(
            &state,
            &Action::CastSpell {
                card: Card::ThassasOracle,
                payment: ManaPool::of(Color::U, 2).into(),
                target: None,
                from: Zone::Hand,
            },
        )
        .unwrap();
        let resolved = pass_twice(&cast);
        // Oracle is on the battlefield and its trigger is on the stack.
        assert_eq!(
            resolved.you.battlefield,
            vec![Permanent::entering(Card::ThassasOracle, false)]
        );
        assert!(resolved.you.battlefield[0].summoning_sick);
        assert!(matches!(
            resolved.stack.as_slice(),
            [StackItem::Trigger {
                kind: TriggerKind::OracleEnters,
                ..
            }]
        ));
        assert_eq!(resolved.outcome, None);
        let won = pass_twice(&resolved);
        assert_eq!(won.outcome, Some(Outcome::Win));
    }

    #[test]
    fn oracle_wins_on_empty_library() {
        let state = with_trigger(
            main_phase(PlayerState {
                battlefield: vec![Permanent::untapped(Card::ThassasOracle)],
                ..PlayerState::default()
            }),
            Card::ThassasOracle,
            TriggerKind::OracleEnters,
        );
        assert_eq!(pass_twice(&state).outcome, Some(Outcome::Win));
    }

    #[test]
    fn stage_three_card_data() {
        assert_eq!(Card::Unearth.kind(), CardKind::Sorcery);
        assert_eq!(Card::YawgmothsWill.kind(), CardKind::Sorcery);
        assert_eq!(Card::FlowState.kind(), CardKind::Sorcery);
        assert_eq!(Card::CabalRitual.kind(), CardKind::Instant);
        assert_eq!(Card::FranticSearch.kind(), CardKind::Instant);
        assert_eq!(Card::Unearth.mana_cost(), ManaCost::colored(Color::B, 1));
        assert_eq!(
            Card::YawgmothsWill.mana_cost(),
            ManaCost::colored(Color::B, 1).with_generic(2)
        );
        assert_eq!(
            Card::FlowState.mana_cost(),
            ManaCost::colored(Color::U, 1).with_generic(1)
        );
        assert_eq!(
            Card::FranticSearch.mana_cost(),
            ManaCost::colored(Color::U, 1).with_generic(2)
        );
        assert_eq!(
            Card::CabalRitual.mana_cost(),
            ManaCost::colored(Color::B, 1).with_generic(1)
        );
        assert_eq!(
            Card::Unearth.cycling_cost(),
            Some(CyclingCost::Mana(ManaCost::FREE.with_generic(2)))
        );
        assert!(!Card::Doomsday.castable());
    }

    #[test]
    fn unearth_targets_cheap_creatures_in_the_graveyard() {
        let state = main_phase(PlayerState {
            graveyard: vec![
                Card::ThassasOracle,
                Card::ThassasOracle,
                Card::LaboratoryManiac,
                Card::DoctorDoom,
                Card::StreetWraith,
                Card::Brainstorm,
            ],
            ..PlayerState::default()
        });
        assert_eq!(
            Card::Unearth.target_options(&state, Player::You),
            Some(vec![
                Target::Graveyard {
                    owner: Player::You,
                    card: Card::ThassasOracle
                },
                Target::Graveyard {
                    owner: Player::You,
                    card: Card::LaboratoryManiac
                }
            ])
        );
        assert_eq!(Card::Brainstorm.target_options(&state, Player::You), None);
        assert_eq!(
            Card::Unearth.target_options(&main_phase(PlayerState::default()), Player::You),
            Some(Vec::new())
        );
    }

    #[test]
    fn unearth_returns_the_target_and_oracle_triggers() {
        let state = main_phase(PlayerState {
            graveyard: vec![Card::Doomsday, Card::ThassasOracle],
            library: vec![Card::Island],
            ..PlayerState::default()
        });
        let state = resolve_targeting(
            state,
            Card::Unearth,
            Some(Target::Graveyard {
                owner: Player::You,
                card: Card::ThassasOracle,
            }),
        );
        assert_eq!(
            state.you.battlefield,
            vec![Permanent::entering(Card::ThassasOracle, false)]
        );
        assert_eq!(state.you.graveyard, vec![Card::Doomsday, Card::Unearth]);
        assert!(matches!(
            state.stack.as_slice(),
            [StackItem::Trigger {
                kind: TriggerKind::OracleEnters,
                ..
            }]
        ));
        // Devotion 2 against one card: the trigger wins.
        assert_eq!(pass_twice(&state).outcome, Some(Outcome::Win));
    }

    #[test]
    fn unearth_does_nothing_when_its_target_has_left_the_graveyard() {
        let state = main_phase(PlayerState {
            graveyard: vec![Card::Doomsday],
            ..PlayerState::default()
        });
        let state = resolve_targeting(
            state,
            Card::Unearth,
            Some(Target::Graveyard {
                owner: Player::You,
                card: Card::ThassasOracle,
            }),
        );
        assert!(state.you.battlefield.is_empty());
        assert!(state.stack.is_empty());
        assert_eq!(state.you.graveyard, vec![Card::Doomsday, Card::Unearth]);
    }

    #[test]
    fn yawgmoths_will_exiles_itself_and_everything_after_it() {
        let state = main_phase(PlayerState {
            graveyard: vec![Card::Doomsday, Card::Brainstorm],
            library: vec![Card::Island, Card::Swamp, Card::Plains],
            battlefield: vec![Permanent::untapped(Card::JaceWielderOfMysteries)],
            ..PlayerState::default()
        });
        let state = resolve(state, Card::YawgmothsWill);
        assert!(state.you.yawgmoths_will);
        assert_eq!(
            state.you.graveyard,
            vec![Card::Doomsday, Card::Brainstorm],
            "what was already there stays"
        );
        assert_eq!(state.you.exile, vec![Card::YawgmothsWill]);

        // A resolved spell and a mill both go to exile now.
        let state = resolve(state, Card::DarkRitual);
        assert_eq!(state.you.exile, vec![Card::YawgmothsWill, Card::DarkRitual]);
        let state = pass_twice(&with_ability(
            state,
            Card::JaceWielderOfMysteries,
            AbilityKind::JacePlusOne {
                target: Player::You,
            },
        ));
        assert_eq!(state.you.hand, vec![Card::Plains]);
        assert_eq!(state.you.graveyard, vec![Card::Doomsday, Card::Brainstorm]);
        assert_eq!(
            state.you.exile,
            vec![
                Card::YawgmothsWill,
                Card::DarkRitual,
                Card::Island,
                Card::Swamp
            ]
        );
    }

    #[test]
    fn flow_state_takes_one_card_without_an_instant_in_the_graveyard() {
        let state = main_phase(PlayerState {
            // A sorcery and an artifact: artifacts don't count.
            graveyard: vec![Card::Doomsday, Card::LotusPetal],
            library: vec![Card::Island, Card::Swamp, Card::ThassasOracle, Card::Forest],
            ..PlayerState::default()
        });
        let state = resolve(state, Card::FlowState);
        assert_eq!(
            state.pending.unwrap().kind,
            ChoiceKind::FlowState { take: 1 }
        );
        // Three picks, two orders for the rest.
        assert_eq!(legal_actions(&state).len(), 6);
        let next = apply(
            &state,
            &Action::Choose(Choice::FlowState {
                to_hand: vec![Card::ThassasOracle],
                bottom: vec![Card::Swamp, Card::Island],
            }),
        )
        .unwrap();
        assert_eq!(next.you.hand, vec![Card::ThassasOracle]);
        assert_eq!(
            next.you.library,
            vec![Card::Forest, Card::Swamp, Card::Island]
        );
        assert!(next.you.library_ordered);
        assert_eq!(next.you.cards_drawn_this_turn, 0, "not a draw");
        assert_eq!(
            next.you.graveyard,
            vec![Card::Doomsday, Card::FlowState, Card::LotusPetal]
        );
        assert_eq!(next.pending, None);
    }

    #[test]
    fn flow_state_takes_two_with_an_instant_and_a_sorcery_in_the_graveyard() {
        let state = main_phase(PlayerState {
            graveyard: vec![Card::Doomsday, Card::DarkRitual],
            library: vec![Card::Island, Card::Swamp, Card::ThassasOracle, Card::Forest],
            ..PlayerState::default()
        });
        let state = resolve(state, Card::FlowState);
        assert_eq!(
            state.pending.unwrap().kind,
            ChoiceKind::FlowState { take: 2 }
        );
        // Three pairs, one card left for the bottom.
        assert_eq!(legal_actions(&state).len(), 3);
        let next = apply(
            &state,
            &Action::Choose(Choice::FlowState {
                to_hand: vec![Card::ThassasOracle, Card::Island],
                bottom: vec![Card::Swamp],
            }),
        )
        .unwrap();
        assert_eq!(next.you.hand, vec![Card::ThassasOracle, Card::Island]);
        assert_eq!(next.you.library, vec![Card::Forest, Card::Swamp]);
    }

    #[test]
    fn flow_state_with_a_short_or_empty_library() {
        let state = main_phase(PlayerState {
            graveyard: vec![Card::Doomsday, Card::Brainstorm],
            library: vec![Card::Island],
            ..PlayerState::default()
        });
        let state = resolve(state, Card::FlowState);
        assert_eq!(
            state.pending.unwrap().kind,
            ChoiceKind::FlowState { take: 1 },
            "two-card mode takes what there is"
        );
        let next = apply(
            &state,
            &Action::Choose(Choice::FlowState {
                to_hand: vec![Card::Island],
                bottom: vec![],
            }),
        )
        .unwrap();
        assert_eq!(next.you.hand, vec![Card::Island]);
        assert!(next.you.library.is_empty());

        let empty = resolve(
            main_phase(PlayerState {
                battlefield: vec![Permanent::untapped(Card::LaboratoryManiac)],
                ..PlayerState::default()
            }),
            Card::FlowState,
        );
        assert_eq!(empty.pending, None);
        assert_eq!(empty.outcome, None, "looking at nothing is not a draw");
        assert_eq!(empty.you.graveyard, vec![Card::FlowState]);
    }

    #[test]
    fn flow_state_on_an_unordered_library() {
        let partial = main_phase(PlayerState {
            library: vec![Card::Island, Card::Swamp, Card::Plains, Card::Forest],
            library_ordered: false,
            ..PlayerState::default()
        });
        assert_eq!(
            resolve(partial, Card::FlowState).outcome,
            Some(Outcome::Loss(LossReason::LibraryUnordered))
        );

        let whole = main_phase(PlayerState {
            library: vec![Card::Island, Card::Swamp, Card::Plains],
            library_ordered: false,
            ..PlayerState::default()
        });
        let state = resolve(whole, Card::FlowState);
        assert_eq!(state.outcome, None);
        let next = apply(
            &state,
            &Action::Choose(Choice::FlowState {
                to_hand: vec![Card::Plains],
                bottom: vec![Card::Island, Card::Swamp],
            }),
        )
        .unwrap();
        assert!(next.you.library_ordered);
        assert_eq!(next.you.library, vec![Card::Island, Card::Swamp]);
    }

    #[test]
    fn cabal_ritual_adds_three_or_five_with_threshold() {
        let six = main_phase(PlayerState {
            graveyard: vec![
                Card::Doomsday,
                Card::Island,
                Card::Island,
                Card::Swamp,
                Card::Swamp,
                Card::Plains,
            ],
            ..PlayerState::default()
        });
        let state = resolve(six.clone(), Card::CabalRitual);
        assert_eq!(state.you.mana_pool, ManaPool::of(Color::B, 3));
        assert_eq!(
            state.you.graveyard.len(),
            7,
            "Cabal Ritual itself arrives after the check"
        );

        let mut seven = six;
        seven.you.graveyard.push(Card::Forest);
        let state = resolve(seven, Card::CabalRitual);
        assert_eq!(state.you.mana_pool, ManaPool::of(Color::B, 5));
    }

    #[test]
    fn frantic_search_draws_two_then_discards_two_then_untaps_up_to_three_lands() {
        let state = main_phase(PlayerState {
            hand: vec![Card::Plains],
            library: vec![Card::ThassasOracle, Card::Swamp, Card::Forest],
            battlefield: vec![
                Permanent::tapped(Card::Island),
                Permanent::tapped(Card::Island),
                Permanent::tapped(Card::UndergroundSea),
                Permanent::tapped(Card::Bayou),
                Permanent::untapped(Card::Swamp),
                Permanent::tapped(Card::MoxSapphire),
            ],
            ..PlayerState::default()
        });
        let state = resolve(state, Card::FranticSearch);
        assert_eq!(
            state.you.hand,
            vec![Card::ThassasOracle, Card::Plains, Card::Swamp]
        );
        assert_eq!(state.you.cards_drawn_this_turn, 2);
        assert_eq!(state.pending.unwrap().kind, ChoiceKind::FranticDiscard);
        // Pairs from {Oracle, Plains, Swamp}.
        assert_eq!(legal_actions(&state).len(), 3);

        let discarded = apply(
            &state,
            &Action::Choose(Choice::Discard(vec![Card::Plains, Card::Swamp])),
        )
        .unwrap();
        assert_eq!(discarded.you.hand, vec![Card::ThassasOracle]);
        assert_eq!(
            discarded.you.graveyard,
            vec![Card::Plains, Card::Swamp],
            "Frantic Search waits for the untap"
        );
        assert_eq!(discarded.pending.unwrap().kind, ChoiceKind::UntapLands);
        // Three of {Island, Island, Sea, Bayou}; the Mox and the untapped
        // Swamp are never offered.
        let options = legal_actions(&discarded);
        assert_eq!(options.len(), 3);
        assert!(!options.iter().any(|a| matches!(
            a,
            Action::Choose(Choice::UntapLands(c))
                if c.contains(&Card::MoxSapphire) || c.contains(&Card::Swamp)
        )));

        let untapped = apply(
            &discarded,
            &Action::Choose(Choice::UntapLands(vec![
                Card::Island,
                Card::UndergroundSea,
                Card::Bayou,
            ])),
        )
        .unwrap();
        assert_eq!(untapped.pending, None);
        assert_eq!(
            untapped.you.graveyard,
            vec![Card::FranticSearch, Card::Plains, Card::Swamp]
        );
        let still_tapped: Vec<Card> = untapped
            .you
            .battlefield
            .iter()
            .filter(|p| p.tapped)
            .map(|p| p.card)
            .collect();
        assert_eq!(still_tapped, vec![Card::MoxSapphire, Card::Island]);
    }

    #[test]
    fn frantic_search_with_a_small_hand_and_nothing_tapped() {
        let state = main_phase(PlayerState {
            library: vec![Card::Island, Card::Swamp],
            battlefield: vec![Permanent::untapped(Card::Forest)],
            ..PlayerState::default()
        });
        let state = resolve(state, Card::FranticSearch);
        assert_eq!(
            legal_actions(&state),
            vec![Action::Choose(Choice::Discard(vec![
                Card::Island,
                Card::Swamp
            ]))]
        );
        let next = apply(
            &state,
            &Action::Choose(Choice::Discard(vec![Card::Island, Card::Swamp])),
        )
        .unwrap();
        assert_eq!(next.pending, None, "no untap step without a tapped land");
        assert!(next.you.hand.is_empty());
        assert_eq!(
            next.you.graveyard,
            vec![Card::FranticSearch, Card::Island, Card::Swamp]
        );
    }

    #[test]
    fn frantic_search_second_draw_into_lab_man_wins_without_a_choice() {
        let state = main_phase(PlayerState {
            library: vec![Card::Island],
            battlefield: vec![Permanent::untapped(Card::LaboratoryManiac)],
            ..PlayerState::default()
        });
        let state = resolve(state, Card::FranticSearch);
        assert_eq!(state.outcome, Some(Outcome::Win));
        assert_eq!(state.pending, None);
    }

    #[test]
    fn frantic_search_discards_are_exiled_under_yawgmoths_will() {
        let state = main_phase(PlayerState {
            library: vec![Card::Island, Card::Swamp],
            yawgmoths_will: true,
            ..PlayerState::default()
        });
        let state = resolve(state, Card::FranticSearch);
        let next = apply(
            &state,
            &Action::Choose(Choice::Discard(vec![Card::Island, Card::Swamp])),
        )
        .unwrap();
        assert!(next.you.graveyard.is_empty());
        assert_eq!(
            next.you.exile,
            vec![Card::FranticSearch, Card::Island, Card::Swamp]
        );
    }

    #[test]
    fn oracle_one_short_scrambles_the_library() {
        let state = with_trigger(
            main_phase(PlayerState {
                battlefield: vec![Permanent::untapped(Card::ThassasOracle)],
                library: vec![Card::Island, Card::Swamp, Card::Plains],
                ..PlayerState::default()
            }),
            Card::ThassasOracle,
            TriggerKind::OracleEnters,
        );
        let after = pass_twice(&state);
        assert_eq!(after.outcome, None);
        assert!(!after.you.library_ordered);
        assert_eq!(after.you.library.len(), 3);
    }

    // ------------------------------------------------------------ stage 4a

    fn on_stack(index: usize, card: Card) -> Target {
        Target::Stack { index, card }
    }

    /// Your `spell` on the stack with the opponent's `counter` aimed at it,
    /// you holding priority.
    fn countered(you: PlayerState, spell: Card, counter: Card) -> State {
        let mut state = main_phase(you);
        state.stack.push(StackItem::Spell {
            card: spell,
            controller: Player::You,
            target: None,
            mode: SpellMode::Normal,
        });
        state.stack.push(StackItem::Spell {
            card: counter,
            controller: Player::Opponent,
            target: Some(on_stack(0, spell)),
            mode: SpellMode::Normal,
        });
        state
    }

    #[test]
    fn counter_data() {
        for card in [
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
        ] {
            assert_eq!(card.kind(), CardKind::Instant, "{card}");
            assert!(card.castable(), "{card}");
            assert!(card.is_blue(), "{card}");
        }
        assert_eq!(Card::Counterspell.mana_cost().to_string(), "{U}{U}");
        assert_eq!(Card::ForceOfWill.mana_cost().to_string(), "{3}{U}{U}");
        assert_eq!(Card::ForceOfNegation.mana_cost().to_string(), "{1}{U}{U}");
        assert_eq!(Card::Daze.mana_cost().to_string(), "{1}{U}");
        assert_eq!(Card::Miscalculation.mana_cost().to_string(), "{1}{U}");
        assert_eq!(Card::SpellPierce.mana_cost().to_string(), "{U}");
        assert_eq!(Card::SpellSnare.mana_cost().to_string(), "{U}");
        assert_eq!(Card::Stifle.mana_cost().to_string(), "{U}");
        assert_eq!(Card::MentalMisstep.mana_cost().to_string(), "{U/P}");
        assert_eq!(Card::PactOfNegation.mana_cost(), ManaCost::FREE);
        assert_eq!(
            Card::Miscalculation.cycling_cost(),
            Some(CyclingCost::Mana(ManaCost::FREE.with_generic(2)))
        );
        // Blue is read off the cost, Phyrexian pips included.
        assert!(Card::GitaxianProbe.is_blue());
        assert!(Card::ThassasOracle.is_blue());
        assert!(!Card::DarkRitual.is_blue());
        assert!(!Card::Island.is_blue());
        assert!(!Card::LionsEyeDiamond.is_blue());
    }

    #[test]
    fn a_countered_spell_goes_to_its_controllers_graveyard() {
        let state = countered(
            PlayerState {
                library: vec![Card::Island, Card::Swamp, Card::Plains],
                ..PlayerState::default()
            },
            Card::Brainstorm,
            Card::Counterspell,
        );
        let after = pass_twice(&state);
        assert!(after.stack.is_empty());
        assert_eq!(after.you.graveyard, vec![Card::Brainstorm]);
        assert_eq!(after.opponent.graveyard, vec![Card::Counterspell]);
        // Nothing was drawn.
        assert_eq!(after.you.library.len(), 3);
        assert!(after.you.hand.is_empty());
        assert_eq!(after.outcome, None);
    }

    #[test]
    fn a_countered_spell_is_exiled_under_yawgmoths_will() {
        let state = countered(
            PlayerState {
                yawgmoths_will: true,
                ..PlayerState::default()
            },
            Card::Brainstorm,
            Card::Counterspell,
        );
        let after = pass_twice(&state);
        assert!(after.you.graveyard.is_empty());
        assert_eq!(after.you.exile, vec![Card::Brainstorm]);
        // Will only replaces *your* graveyard.
        assert_eq!(after.opponent.graveyard, vec![Card::Counterspell]);
    }

    #[test]
    fn force_of_negation_exiles_what_it_counters() {
        let after = pass_twice(&countered(
            PlayerState::default(),
            Card::Brainstorm,
            Card::ForceOfNegation,
        ));
        assert!(after.you.graveyard.is_empty());
        assert_eq!(after.you.exile, vec![Card::Brainstorm]);
        assert_eq!(after.opponent.graveyard, vec![Card::ForceOfNegation]);
    }

    #[test]
    fn hard_counters_resolve_the_same_way() {
        for counter in [
            Card::ForceOfWill,
            Card::MentalMisstep,
            Card::SpellSnare,
            Card::PactOfNegation,
        ] {
            let after = pass_twice(&countered(
                PlayerState::default(),
                Card::Brainstorm,
                counter,
            ));
            assert!(after.stack.is_empty(), "{counter}");
            assert_eq!(after.you.graveyard, vec![Card::Brainstorm], "{counter}");
            assert_eq!(after.opponent.graveyard, vec![counter], "{counter}");
            assert_eq!(after.pending, None, "{counter}");
        }
    }

    #[test]
    fn a_counter_whose_target_is_gone_does_nothing() {
        // Two Counterspells on one Brainstorm: the first counters it, the
        // second finds no target and just goes to the graveyard.
        let mut state = countered(PlayerState::default(), Card::Brainstorm, Card::Counterspell);
        state.stack.push(StackItem::Spell {
            card: Card::Counterspell,
            controller: Player::Opponent,
            target: Some(on_stack(0, Card::Brainstorm)),
            mode: SpellMode::Normal,
        });
        let after_first = pass_twice(&state);
        assert_eq!(
            after_first.stack,
            vec![StackItem::Spell {
                card: Card::Counterspell,
                controller: Player::Opponent,
                target: None,
                mode: SpellMode::Normal,
            }]
        );
        let after_second = pass_twice(&after_first);
        assert!(after_second.stack.is_empty());
        assert_eq!(after_second.you.graveyard, vec![Card::Brainstorm]);
        assert_eq!(
            after_second.opponent.graveyard,
            vec![Card::Counterspell, Card::Counterspell]
        );
    }

    #[test]
    fn stifle_counters_abilities_and_triggers_but_their_costs_stay_paid() {
        // Oracle's trigger: no win, Oracle stays.
        let mut state = with_trigger(
            main_phase(PlayerState {
                battlefield: vec![Permanent::untapped(Card::ThassasOracle)],
                library: vec![Card::Island],
                ..PlayerState::default()
            }),
            Card::ThassasOracle,
            TriggerKind::OracleEnters,
        );
        state.stack.push(StackItem::Spell {
            card: Card::Stifle,
            controller: Player::Opponent,
            target: Some(on_stack(0, Card::ThassasOracle)),
            mode: SpellMode::Normal,
        });
        let after = pass_twice(&state);
        assert!(after.stack.is_empty());
        assert_eq!(after.outcome, None);
        assert_eq!(after.you.battlefield.len(), 1);
        assert_eq!(after.opponent.graveyard, vec![Card::Stifle]);
        // Cycling: the card is already discarded and no card is drawn.
        let mut state = with_ability(
            main_phase(PlayerState {
                graveyard: vec![Card::StreetWraith],
                library: vec![Card::Island],
                ..PlayerState::default()
            }),
            Card::StreetWraith,
            AbilityKind::Cycling,
        );
        state.stack.push(StackItem::Spell {
            card: Card::Stifle,
            controller: Player::Opponent,
            target: Some(on_stack(0, Card::StreetWraith)),
            mode: SpellMode::Normal,
        });
        let after = pass_twice(&state);
        assert!(after.stack.is_empty());
        assert!(after.you.hand.is_empty());
        assert_eq!(after.you.library, vec![Card::Island]);
        assert_eq!(after.you.graveyard, vec![Card::StreetWraith]);
    }

    #[test]
    fn daze_lets_you_pay_one_from_your_pool_or_lose_the_spell() {
        let state = countered(
            PlayerState {
                mana_pool: ManaPool::of(Color::U, 1).with(Color::B, 1),
                library: vec![Card::Island, Card::Swamp, Card::Plains],
                ..PlayerState::default()
            },
            Card::Brainstorm,
            Card::Daze,
        );
        let taxed = pass_twice(&state);
        let pending = taxed.pending.expect("your decision");
        assert_eq!(pending.player, Player::You);
        assert_eq!(pending.bin_owner, Player::Opponent);
        assert_eq!(
            pending.kind,
            ChoiceKind::PayTax {
                amount: 1,
                target: on_stack(0, Card::Brainstorm),
            }
        );
        assert_eq!(
            legal_actions(&taxed),
            vec![
                Action::Choose(Choice::PayTax(ManaPool::of(Color::B, 1))),
                Action::Choose(Choice::PayTax(ManaPool::of(Color::U, 1))),
                Action::Choose(Choice::DeclineTax),
            ]
        );
        // Pay with the B: Brainstorm stays, Daze goes to the opponent's
        // graveyard, you keep priority.
        let paid = apply(
            &taxed,
            &Action::Choose(Choice::PayTax(ManaPool::of(Color::B, 1))),
        )
        .unwrap();
        assert_eq!(paid.you.mana_pool, ManaPool::of(Color::U, 1));
        assert_eq!(paid.stack.len(), 1);
        assert!(paid.you.graveyard.is_empty());
        assert_eq!(paid.opponent.graveyard, vec![Card::Daze]);
        assert_eq!(paid.priority, Player::You);
        assert_eq!(pass_twice(&paid).you.hand.len(), 3);
        // Decline: Brainstorm is countered.
        let declined = apply(&taxed, &Action::Choose(Choice::DeclineTax)).unwrap();
        assert!(declined.stack.is_empty());
        assert_eq!(declined.you.graveyard, vec![Card::Brainstorm]);
        assert_eq!(declined.opponent.graveyard, vec![Card::Daze]);
        assert_eq!(
            declined.you.mana_pool,
            ManaPool::of(Color::U, 1).with(Color::B, 1)
        );
    }

    #[test]
    fn spell_pierce_and_miscalculation_ask_for_two() {
        for counter in [Card::SpellPierce, Card::Miscalculation] {
            let state = countered(
                PlayerState {
                    mana_pool: ManaPool::of(Color::U, 2),
                    ..PlayerState::default()
                },
                Card::Brainstorm,
                counter,
            );
            let taxed = pass_twice(&state);
            assert_eq!(
                legal_actions(&taxed),
                vec![
                    Action::Choose(Choice::PayTax(ManaPool::of(Color::U, 2))),
                    Action::Choose(Choice::DeclineTax),
                ],
                "{counter}"
            );
            // One mana short: only declining is left.
            let short = countered(
                PlayerState {
                    mana_pool: ManaPool::of(Color::U, 1),
                    ..PlayerState::default()
                },
                Card::Brainstorm,
                counter,
            );
            assert_eq!(
                legal_actions(&pass_twice(&short)),
                vec![Action::Choose(Choice::DeclineTax)],
                "{counter}"
            );
        }
    }

    #[test]
    fn a_tax_spell_whose_target_is_gone_asks_nothing() {
        let mut state = main_phase(PlayerState::default());
        state.stack.push(StackItem::Spell {
            card: Card::Daze,
            controller: Player::Opponent,
            target: None,
            mode: SpellMode::Normal,
        });
        let after = pass_twice(&state);
        assert_eq!(after.pending, None);
        assert_eq!(after.opponent.graveyard, vec![Card::Daze]);
    }

    // ------------------------------------------------------------ stage 4b

    fn their_spell(card: Card, target: Option<Target>) -> StackItem {
        StackItem::Spell {
            card,
            controller: Player::Opponent,
            target,
            mode: SpellMode::Normal,
        }
    }

    fn your_spell(card: Card) -> StackItem {
        StackItem::Spell {
            card,
            controller: Player::You,
            target: None,
            mode: SpellMode::Normal,
        }
    }

    fn your_permanent(card: Card) -> Target {
        Target::Permanent {
            owner: Player::You,
            card,
        }
    }

    fn your_graveyard(card: Card) -> Target {
        Target::Graveyard {
            owner: Player::You,
            card,
        }
    }

    #[test]
    fn removal_and_odd_counter_data() {
        for card in [
            Card::Pyroblast,
            Card::Hydroblast,
            Card::SwordsToPlowshares,
            Card::SnuffOut,
            Card::SurgicalExtraction,
            Card::Flusterstorm,
            Card::ConsignToMemory,
            Card::MindbreakTrap,
        ] {
            assert_eq!(card.kind(), CardKind::Instant, "{card}");
            assert!(card.castable(), "{card}");
        }
        assert_eq!(Card::Pyroblast.mana_cost().to_string(), "{R}");
        assert_eq!(Card::Hydroblast.mana_cost().to_string(), "{U}");
        assert_eq!(Card::SwordsToPlowshares.mana_cost().to_string(), "{W}");
        assert_eq!(Card::SnuffOut.mana_cost().to_string(), "{3}{B}");
        assert_eq!(Card::SurgicalExtraction.mana_cost().to_string(), "{B/P}");
        assert_eq!(Card::Flusterstorm.mana_cost().to_string(), "{U}");
        assert_eq!(Card::ConsignToMemory.mana_cost().to_string(), "{U}");
        assert_eq!(Card::MindbreakTrap.mana_cost().to_string(), "{2}{U}{U}");
        assert!(Card::Pyroblast.is_red() && !Card::Pyroblast.is_blue());
        assert!(Card::Hydroblast.is_blue() && !Card::Hydroblast.is_red());
        for black in [
            Card::DoctorDoom,
            Card::StreetWraith,
            Card::SnuffOut,
            Card::SurgicalExtraction,
        ] {
            assert!(black.is_black(), "{black}");
        }
        assert!(!Card::ThassasOracle.is_black());
        assert!(!Card::LaboratoryManiac.is_black());
        for colorless in [
            Card::LionsEyeDiamond,
            Card::BlackLotus,
            Card::LotusPetal,
            Card::MoxSapphire,
            Card::MoxJet,
        ] {
            assert!(colorless.is_colorless(), "{colorless}");
        }
        assert!(!Card::PactOfNegation.is_colorless());
        assert!(!Card::Brainstorm.is_colorless());
        assert_eq!(
            Card::ConsignToMemory.replicate_cost(),
            Some(ManaCost::FREE.with_generic(1))
        );
        assert_eq!(Card::Flusterstorm.replicate_cost(), None);
        assert!(Card::Flusterstorm.has_storm());
        assert!(!Card::ConsignToMemory.has_storm());
    }

    #[test]
    fn pyroblast_destroys_oracle_before_its_trigger_resolves() {
        let mut state = with_trigger(
            main_phase(PlayerState {
                battlefield: vec![Permanent::untapped(Card::ThassasOracle)],
                library: vec![Card::Island],
                ..PlayerState::default()
            }),
            Card::ThassasOracle,
            TriggerKind::OracleEnters,
        );
        state.stack.push(their_spell(
            Card::Pyroblast,
            Some(your_permanent(Card::ThassasOracle)),
        ));
        let after = pass_twice(&state);
        assert!(after.you.battlefield.is_empty());
        assert_eq!(after.you.graveyard, vec![Card::ThassasOracle]);
        assert_eq!(after.opponent.graveyard, vec![Card::Pyroblast]);
        assert_eq!(after.stack.len(), 1);
        // Devotion is now 0 against one card: no win.
        let resolved = pass_twice(&after);
        assert_eq!(resolved.outcome, None);
        assert_eq!(resolved.you.library, vec![Card::Island]);
    }

    #[test]
    fn pyroblast_counters_a_blue_spell() {
        let after = pass_twice(&countered(
            PlayerState::default(),
            Card::ThassasOracle,
            Card::Pyroblast,
        ));
        assert!(after.stack.is_empty());
        assert_eq!(after.you.graveyard, vec![Card::ThassasOracle]);
        assert_eq!(after.opponent.graveyard, vec![Card::Pyroblast]);
    }

    #[test]
    fn swords_exiles_and_snuff_out_destroys() {
        let lab_man = || {
            main_phase(PlayerState {
                battlefield: vec![Permanent::untapped(Card::LaboratoryManiac)],
                ..PlayerState::default()
            })
        };
        let mut state = lab_man();
        state.stack.push(their_spell(
            Card::SwordsToPlowshares,
            Some(your_permanent(Card::LaboratoryManiac)),
        ));
        let after = pass_twice(&state);
        assert!(after.you.battlefield.is_empty());
        assert_eq!(after.you.exile, vec![Card::LaboratoryManiac]);
        assert!(after.you.graveyard.is_empty());
        // The life gain is ignored.
        assert_eq!(after.you.life, 20);

        let mut state = lab_man();
        state.stack.push(their_spell(
            Card::SnuffOut,
            Some(your_permanent(Card::LaboratoryManiac)),
        ));
        let after = pass_twice(&state);
        assert!(after.you.battlefield.is_empty());
        assert_eq!(after.you.graveyard, vec![Card::LaboratoryManiac]);

        let mut state = lab_man();
        state.you.yawgmoths_will = true;
        state.stack.push(their_spell(
            Card::SnuffOut,
            Some(your_permanent(Card::LaboratoryManiac)),
        ));
        let after = pass_twice(&state);
        assert!(after.you.graveyard.is_empty());
        assert_eq!(after.you.exile, vec![Card::LaboratoryManiac]);
    }

    #[test]
    fn removal_on_a_permanent_that_left_does_nothing() {
        let mut state = main_phase(PlayerState::default());
        state.stack.push(their_spell(
            Card::SwordsToPlowshares,
            Some(your_permanent(Card::LaboratoryManiac)),
        ));
        let after = pass_twice(&state);
        assert!(after.you.exile.is_empty());
        assert_eq!(after.opponent.graveyard, vec![Card::SwordsToPlowshares]);
    }

    #[test]
    fn lab_man_removed_in_response_turns_the_empty_draw_into_a_loss() {
        let mut state = main_phase(PlayerState {
            battlefield: vec![Permanent::untapped(Card::LaboratoryManiac)],
            library: vec![Card::Island],
            ..PlayerState::default()
        });
        state.stack.push(your_spell(Card::Brainstorm));
        state.stack.push(their_spell(
            Card::SnuffOut,
            Some(your_permanent(Card::LaboratoryManiac)),
        ));
        let after = pass_twice(&pass_twice(&state));
        assert_eq!(
            after.outcome,
            Some(Outcome::Loss(LossReason::DrewFromEmptyLibrary))
        );
    }

    #[test]
    fn surgical_exiles_every_copy_from_graveyard_hand_and_library_then_shuffles() {
        let mut state = main_phase(PlayerState {
            graveyard: vec![Card::Doomsday, Card::ThassasOracle],
            hand: vec![Card::ThassasOracle, Card::Brainstorm],
            library: vec![Card::ThassasOracle, Card::Island, Card::Swamp],
            ..PlayerState::default()
        });
        state.stack.push(their_spell(
            Card::SurgicalExtraction,
            Some(your_graveyard(Card::ThassasOracle)),
        ));
        let after = pass_twice(&state);
        assert_eq!(after.you.graveyard, vec![Card::Doomsday]);
        assert_eq!(after.you.hand, vec![Card::Brainstorm]);
        assert_eq!(after.you.library, vec![Card::Island, Card::Swamp]);
        assert!(!after.you.library_ordered);
        assert_eq!(after.you.exile, vec![Card::ThassasOracle; 3]);
        assert_eq!(after.opponent.graveyard, vec![Card::SurgicalExtraction]);
    }

    #[test]
    fn surgical_on_doomsday_only_scrambles_a_library_of_two_or_more() {
        let mut state = main_phase(PlayerState {
            graveyard: vec![Card::Doomsday],
            library: vec![Card::ThassasOracle],
            ..PlayerState::default()
        });
        state.stack.push(their_spell(
            Card::SurgicalExtraction,
            Some(your_graveyard(Card::Doomsday)),
        ));
        let after = pass_twice(&state);
        assert!(after.you.graveyard.is_empty());
        assert_eq!(after.you.exile, vec![Card::Doomsday]);
        assert!(after.you.library_ordered);
    }

    #[test]
    fn surgical_fizzles_when_the_card_has_left_the_graveyard_and_beats_unearth() {
        // Surgical above Unearth: Oracle is exiled first, Unearth finds nothing.
        let mut state = main_phase(PlayerState {
            graveyard: vec![Card::Doomsday, Card::ThassasOracle],
            library: vec![Card::Island, Card::Swamp],
            ..PlayerState::default()
        });
        state.stack.push(StackItem::Spell {
            card: Card::Unearth,
            controller: Player::You,
            target: Some(your_graveyard(Card::ThassasOracle)),
            mode: SpellMode::Normal,
        });
        state.stack.push(their_spell(
            Card::SurgicalExtraction,
            Some(your_graveyard(Card::ThassasOracle)),
        ));
        let after = pass_twice(&pass_twice(&state));
        assert!(after.you.battlefield.is_empty());
        assert_eq!(after.you.graveyard, vec![Card::Doomsday, Card::Unearth]);
        assert_eq!(after.you.exile, vec![Card::ThassasOracle]);
        assert!(!after.you.library_ordered);

        // Unearth above Surgical: Oracle returns, Surgical fizzles, no shuffle.
        let mut state = main_phase(PlayerState {
            graveyard: vec![Card::Doomsday, Card::ThassasOracle],
            library: vec![Card::Island, Card::Swamp],
            ..PlayerState::default()
        });
        state.stack.push(their_spell(
            Card::SurgicalExtraction,
            Some(your_graveyard(Card::ThassasOracle)),
        ));
        state.stack.push(StackItem::Spell {
            card: Card::Unearth,
            controller: Player::You,
            target: Some(your_graveyard(Card::ThassasOracle)),
            mode: SpellMode::Normal,
        });
        let after = pass_twice(&state);
        assert_eq!(after.you.battlefield.len(), 1);
        // Oracle's trigger is on top; resolve it (devotion 2 for two cards wins).
        let after = pass_twice(&after);
        assert_eq!(after.outcome, Some(Outcome::Win));
        let mut no_win = after.clone();
        no_win.outcome = None;
        let after = pass_twice(&no_win);
        assert!(after.you.exile.is_empty());
        assert!(after.you.library_ordered);
        assert_eq!(after.opponent.graveyard, vec![Card::SurgicalExtraction]);
    }

    #[test]
    fn flusterstorm_copies_each_tax_one_and_never_touch_a_graveyard() {
        let mut state = main_phase(PlayerState {
            mana_pool: ManaPool::of(Color::U, 1),
            ..PlayerState::default()
        });
        state.stack.push(your_spell(Card::DarkRitual));
        state.stack.push(their_spell(
            Card::Flusterstorm,
            Some(on_stack(0, Card::DarkRitual)),
        ));
        state.stack.push(StackItem::Trigger {
            source: Card::Flusterstorm,
            kind: TriggerKind::Copies { count: 2 },
            controller: Player::Opponent,
        });
        // One legal target: the copies appear without a choice.
        let copied = pass_twice(&state);
        assert_eq!(copied.pending, None);
        assert_eq!(copied.stack.len(), 4);
        for item in &copied.stack[2..] {
            assert_eq!(
                *item,
                StackItem::Spell {
                    card: Card::Flusterstorm,
                    controller: Player::Opponent,
                    target: Some(on_stack(0, Card::DarkRitual)),
                    mode: SpellMode::Copy,
                }
            );
        }
        // First copy: pay the U.
        let taxed = pass_twice(&copied);
        let pending = taxed.pending.expect("your decision");
        assert_eq!(pending.bin, None);
        assert_eq!(
            pending.kind,
            ChoiceKind::PayTax {
                amount: 1,
                target: on_stack(0, Card::DarkRitual),
            }
        );
        let paid = apply(
            &taxed,
            &Action::Choose(Choice::PayTax(ManaPool::of(Color::U, 1))),
        )
        .unwrap();
        assert_eq!(paid.stack.len(), 3);
        assert!(paid.opponent.graveyard.is_empty());
        // Second copy: nothing left to pay, Dark Ritual is countered.
        let taxed = pass_twice(&paid);
        assert_eq!(
            legal_actions(&taxed),
            vec![Action::Choose(Choice::DeclineTax)]
        );
        let declined = apply(&taxed, &Action::Choose(Choice::DeclineTax)).unwrap();
        assert_eq!(declined.you.graveyard, vec![Card::DarkRitual]);
        assert_eq!(declined.stack, vec![their_spell(Card::Flusterstorm, None)]);
        assert!(declined.opponent.graveyard.is_empty());
        // The original fizzles and is the only Flusterstorm ever binned.
        let done = pass_twice(&declined);
        assert!(done.stack.is_empty());
        assert_eq!(done.opponent.graveyard, vec![Card::Flusterstorm]);
    }

    #[test]
    fn copies_with_several_legal_targets_are_spread_by_the_caster() {
        let mut state = main_phase(PlayerState::default());
        state.stack.push(your_spell(Card::Brainstorm));
        state.stack.push(your_spell(Card::DarkRitual));
        state.stack.push(their_spell(
            Card::Flusterstorm,
            Some(on_stack(0, Card::Brainstorm)),
        ));
        state.stack.push(StackItem::Trigger {
            source: Card::Flusterstorm,
            kind: TriggerKind::Copies { count: 3 },
            controller: Player::Opponent,
        });
        let asked = pass_twice(&state);
        let pending = asked.pending.expect("the opponent spreads the copies");
        assert_eq!(pending.player, Player::Opponent);
        assert_eq!(pending.kind, ChoiceKind::CopyTargets { count: 3 });
        let options = legal_actions(&asked);
        // Two targets, three copies: 3+0, 2+1, 1+2, 0+3.
        assert_eq!(options.len(), 4);
        let spread = Choice::CopyTargets(vec![
            on_stack(0, Card::Brainstorm),
            on_stack(0, Card::Brainstorm),
            on_stack(1, Card::DarkRitual),
        ]);
        assert!(options.contains(&Action::Choose(spread.clone())));
        let after = apply(&asked, &Action::Choose(spread)).unwrap();
        assert_eq!(after.stack.len(), 6);
        let copy_targets: Vec<Option<Target>> = after.stack[3..]
            .iter()
            .map(|item| match item {
                StackItem::Spell {
                    target,
                    mode: SpellMode::Copy,
                    ..
                } => *target,
                other => panic!("expected a copy, got {other:?}"),
            })
            .collect();
        assert_eq!(
            copy_targets,
            vec![
                Some(on_stack(0, Card::Brainstorm)),
                Some(on_stack(0, Card::Brainstorm)),
                Some(on_stack(1, Card::DarkRitual)),
            ]
        );
        // The wrong size or an illegal target is rejected.
        assert!(matches!(
            apply(
                &asked,
                &Action::Choose(Choice::CopyTargets(vec![on_stack(0, Card::Brainstorm)]))
            ),
            Err(EngineError::IllegalAction(_))
        ));
    }

    #[test]
    fn stifling_the_storm_trigger_leaves_only_the_original() {
        let mut state = main_phase(PlayerState::default());
        state.stack.push(your_spell(Card::DarkRitual));
        state.stack.push(their_spell(
            Card::Flusterstorm,
            Some(on_stack(0, Card::DarkRitual)),
        ));
        state.stack.push(StackItem::Trigger {
            source: Card::Flusterstorm,
            kind: TriggerKind::Copies { count: 4 },
            controller: Player::Opponent,
        });
        state.stack.push(StackItem::Spell {
            card: Card::Stifle,
            controller: Player::You,
            target: Some(on_stack(2, Card::Flusterstorm)),
            mode: SpellMode::Normal,
        });
        let after = pass_twice(&state);
        assert_eq!(after.stack.len(), 2);
        assert_eq!(after.you.graveyard, vec![Card::Stifle]);
    }

    #[test]
    fn a_copy_trigger_with_no_legal_target_makes_no_copies() {
        let mut state = main_phase(PlayerState::default());
        state.stack.push(their_spell(Card::Flusterstorm, None));
        state.stack.push(StackItem::Trigger {
            source: Card::Flusterstorm,
            kind: TriggerKind::Copies { count: 2 },
            controller: Player::Opponent,
        });
        let after = pass_twice(&state);
        assert_eq!(after.stack, vec![their_spell(Card::Flusterstorm, None)]);
        assert_eq!(after.pending, None);
    }

    #[test]
    fn consign_counters_a_trigger_or_a_colorless_spell() {
        let state = with_trigger(
            main_phase(PlayerState {
                battlefield: vec![Permanent::untapped(Card::ThassasOracle)],
                ..PlayerState::default()
            }),
            Card::ThassasOracle,
            TriggerKind::OracleEnters,
        );
        let mut state = state;
        state.stack.push(their_spell(
            Card::ConsignToMemory,
            Some(on_stack(0, Card::ThassasOracle)),
        ));
        let after = pass_twice(&state);
        assert!(after.stack.is_empty());
        assert_eq!(after.outcome, None);
        assert_eq!(after.opponent.graveyard, vec![Card::ConsignToMemory]);

        let after = pass_twice(&countered(
            PlayerState::default(),
            Card::LionsEyeDiamond,
            Card::ConsignToMemory,
        ));
        assert!(after.you.battlefield.is_empty());
        assert_eq!(after.you.graveyard, vec![Card::LionsEyeDiamond]);
    }

    #[test]
    fn mindbreak_trap_exiles_every_spell_you_control_and_nothing_else() {
        let mut state = main_phase(PlayerState::default());
        state.stack.push(your_spell(Card::Brainstorm));
        state.stack.push(your_spell(Card::DarkRitual));
        state.stack.push(their_spell(
            Card::Counterspell,
            Some(on_stack(0, Card::Brainstorm)),
        ));
        state.stack.push(their_spell(
            Card::MindbreakTrap,
            Some(on_stack(1, Card::DarkRitual)),
        ));
        let after = pass_twice(&state);
        assert_eq!(after.you.exile, vec![Card::Brainstorm, Card::DarkRitual]);
        assert!(after.you.graveyard.is_empty());
        // The Counterspell stays, with its target gone.
        assert_eq!(after.stack, vec![their_spell(Card::Counterspell, None)]);
        assert_eq!(after.opponent.graveyard, vec![Card::MindbreakTrap]);
    }

    // ------------------------------------------------------------ stage 4c

    use crate::state::test_support::versus;

    fn opponent_hand(hand: Vec<Card>) -> PlayerState {
        PlayerState {
            hand,
            ..PlayerState::default()
        }
    }

    #[test]
    fn protection_and_discard_data() {
        assert_eq!(Card::Silence.kind(), CardKind::Instant);
        assert_eq!(Card::VeilOfSummer.kind(), CardKind::Instant);
        assert_eq!(Card::Teferi.kind(), CardKind::Planeswalker);
        assert_eq!(Card::DefenseGrid.kind(), CardKind::Artifact);
        assert_eq!(Card::Thoughtseize.kind(), CardKind::Sorcery);
        assert_eq!(Card::Duress.kind(), CardKind::Sorcery);
        assert_eq!(Card::Grief.kind(), CardKind::Creature);
        assert_eq!(Card::Silence.mana_cost().to_string(), "{W}");
        assert_eq!(Card::VeilOfSummer.mana_cost().to_string(), "{G}");
        assert_eq!(Card::Teferi.mana_cost().to_string(), "{1}{W}{U}");
        assert_eq!(Card::DefenseGrid.mana_cost().to_string(), "{2}");
        assert_eq!(Card::Thoughtseize.mana_cost().to_string(), "{B}");
        assert_eq!(Card::Duress.mana_cost().to_string(), "{B}");
        assert_eq!(Card::Grief.mana_cost().to_string(), "{2}{B}{B}");
        assert_eq!(Card::Teferi.starting_loyalty(), 4);
        assert_eq!(Card::Teferi.blue_devotion(), 1);
        assert!(Card::Grief.is_black());
        assert!(Card::Grief.evoke_pitch().is_some());
        assert!(Card::ThassasOracle.evoke_pitch().is_none());
        assert_eq!(Card::Grief.etb_trigger(), Some(TriggerKind::GriefEnters));
        assert_eq!(Card::DefenseGrid.mana_ability(), None);
    }

    #[test]
    fn silence_stops_the_opponent_from_casting() {
        let state = versus(
            PlayerState {
                hand: vec![Card::ThassasOracle],
                mana_pool: ManaPool::of(Color::U, 2),
                ..PlayerState::default()
            },
            PlayerState {
                hand: vec![Card::Counterspell],
                battlefield: vec![Permanent::untapped(Card::Island); 2],
                ..PlayerState::default()
            },
        );
        let silenced = resolve(state, Card::Silence);
        assert!(silenced.opponent.silenced);
        assert!(!silenced.you.silenced);
        assert_eq!(silenced.you.graveyard, vec![Card::Silence]);
        // Oracle on the stack, opponent's turn to act: pass is all they have.
        let mut state = silenced;
        state.stack.push(your_spell(Card::ThassasOracle));
        let theirs = apply(&state, &Action::Pass).unwrap();
        assert_eq!(legal_actions(&theirs), vec![Action::Pass]);
    }

    #[test]
    fn veil_draws_only_after_a_blue_or_black_spell_and_then_protects() {
        // No blue or black spell from them yet: no draw.
        let state = main_phase(PlayerState {
            library: vec![Card::Island],
            ..PlayerState::default()
        });
        let after = resolve(state, Card::VeilOfSummer);
        assert!(after.you.hand.is_empty());
        assert!(after.you.veiled);
        // They have: draw one.
        let mut state = main_phase(PlayerState {
            library: vec![Card::Island],
            ..PlayerState::default()
        });
        state.opponent.cast_blue_or_black_this_turn = true;
        let after = resolve(state, Card::VeilOfSummer);
        assert_eq!(after.you.hand, vec![Card::Island]);
        assert_eq!(after.you.cards_drawn_this_turn, 1);
    }

    #[test]
    fn a_veiled_spell_cannot_be_countered_or_taxed_but_can_be_exiled() {
        let veiled = || PlayerState {
            veiled: true,
            ..PlayerState::default()
        };
        // Counterspell resolves without effect and is still binned.
        let after = pass_twice(&countered(
            veiled(),
            Card::ThassasOracle,
            Card::Counterspell,
        ));
        assert_eq!(after.stack, vec![your_spell(Card::ThassasOracle)]);
        assert_eq!(after.opponent.graveyard, vec![Card::Counterspell]);
        // Daze asks for nothing.
        let after = pass_twice(&countered(veiled(), Card::Brainstorm, Card::Daze));
        assert_eq!(after.pending, None);
        assert_eq!(after.stack, vec![your_spell(Card::Brainstorm)]);
        assert_eq!(after.opponent.graveyard, vec![Card::Daze]);
        // Mindbreak Trap exiles anyway.
        let after = pass_twice(&countered(veiled(), Card::Brainstorm, Card::MindbreakTrap));
        assert!(after.stack.is_empty());
        assert_eq!(after.you.exile, vec![Card::Brainstorm]);
        // And counters are not even offered against a veiled player's spells.
        let mut state = versus(veiled(), opponent_hand(vec![Card::Counterspell]));
        state.opponent.battlefield = vec![Permanent::untapped(Card::Island); 2];
        state.stack.push(your_spell(Card::Brainstorm));
        assert_eq!(
            Card::Counterspell.target_options(&state, Player::Opponent),
            Some(Vec::new())
        );
        assert_eq!(
            Card::MindbreakTrap.target_options(&state, Player::Opponent),
            Some(vec![on_stack(0, Card::Brainstorm)])
        );
    }

    #[test]
    fn veil_gives_hexproof_from_blue_and_black_only() {
        let mut state = main_phase(PlayerState {
            veiled: true,
            battlefield: vec![Permanent::untapped(Card::LaboratoryManiac)],
            ..PlayerState::default()
        });
        let lab_man = your_permanent(Card::LaboratoryManiac);
        assert_eq!(
            Card::SnuffOut.target_options(&state, Player::Opponent),
            Some(Vec::new())
        );
        assert_eq!(
            Card::Pyroblast.target_options(&state, Player::Opponent),
            Some(vec![lab_man])
        );
        assert_eq!(
            Card::SwordsToPlowshares.target_options(&state, Player::Opponent),
            Some(vec![lab_man])
        );
        // A Snuff Out already on the stack fizzles once Veil has resolved.
        state.you.veiled = false;
        state.stack.push(their_spell(Card::SnuffOut, Some(lab_man)));
        state.stack.push(your_spell(Card::VeilOfSummer));
        let after = pass_twice(&pass_twice(&state));
        assert!(after.stack.is_empty());
        assert_eq!(after.you.battlefield.len(), 1);
        assert_eq!(after.opponent.graveyard, vec![Card::SnuffOut]);
        // Your own targeting is unaffected: Teferi's bounce still works.
    }

    #[test]
    fn teferi_plus_one_gives_sorceries_flash() {
        let state = with_ability(
            main_phase(PlayerState {
                battlefield: vec![Permanent::untapped(Card::Teferi)],
                ..PlayerState::default()
            }),
            Card::Teferi,
            AbilityKind::TeferiPlusOne,
        );
        let after = pass_twice(&state);
        assert!(after.you.sorceries_have_flash);
    }

    #[test]
    fn teferi_minus_three_bounces_and_draws() {
        let mut state = versus(
            PlayerState {
                battlefield: vec![
                    Permanent::untapped(Card::Teferi),
                    Permanent::untapped(Card::LionsEyeDiamond),
                ],
                library: vec![Card::Island],
                ..PlayerState::default()
            },
            PlayerState {
                battlefield: vec![
                    Permanent::untapped(Card::LaboratoryManiac),
                    Permanent::untapped(Card::Swamp),
                ],
                ..PlayerState::default()
            },
        );
        // Options: no target, your LED, their Lab Man (no lands, no Teferi).
        assert_eq!(
            Card::Teferi.loyalty_abilities(&state, Player::You),
            vec![
                LoyaltyAbility::TeferiPlusOne,
                LoyaltyAbility::TeferiMinusThree { target: None },
                LoyaltyAbility::TeferiMinusThree {
                    target: Some(your_permanent(Card::LionsEyeDiamond)),
                },
                LoyaltyAbility::TeferiMinusThree {
                    target: Some(Target::Permanent {
                        owner: Player::Opponent,
                        card: Card::LaboratoryManiac,
                    }),
                },
            ]
        );
        state.stack.push(StackItem::Ability {
            source: Card::Teferi,
            kind: AbilityKind::TeferiMinusThree {
                target: Some(your_permanent(Card::LionsEyeDiamond)),
            },
            controller: Player::You,
        });
        let after = pass_twice(&state);
        assert_eq!(after.you.hand, vec![Card::LionsEyeDiamond, Card::Island]);
        assert_eq!(after.you.battlefield.len(), 1);
        assert!(after.you.library.is_empty());
        // With no target it just draws; into an empty library with Lab Man
        // that is the win.
        let mut state = main_phase(PlayerState {
            battlefield: vec![
                Permanent::untapped(Card::Teferi),
                Permanent::untapped(Card::LaboratoryManiac),
            ],
            ..PlayerState::default()
        });
        state.stack.push(StackItem::Ability {
            source: Card::Teferi,
            kind: AbilityKind::TeferiMinusThree { target: None },
            controller: Player::You,
        });
        assert_eq!(pass_twice(&state).outcome, Some(Outcome::Win));
    }

    #[test]
    fn thoughtseize_and_duress_pick_from_the_opponents_hand() {
        let hand = vec![
            Card::Counterspell,
            Card::Counterspell,
            Card::ThassasOracle,
            Card::Island,
        ];
        let state = versus(PlayerState::default(), opponent_hand(hand.clone()));
        let asked = resolve(state, Card::Thoughtseize);
        assert_eq!(asked.you.life, 18);
        let pending = asked.pending.expect("your pick");
        assert_eq!(pending.player, Player::You);
        assert_eq!(pending.bin, Some(Card::Thoughtseize));
        assert_eq!(
            legal_actions(&asked),
            vec![
                Action::Choose(Choice::Discard(vec![Card::ThassasOracle])),
                Action::Choose(Choice::Discard(vec![Card::Counterspell])),
            ]
        );
        let after = apply(
            &asked,
            &Action::Choose(Choice::Discard(vec![Card::Counterspell])),
        )
        .unwrap();
        assert_eq!(
            after.opponent.hand,
            vec![Card::ThassasOracle, Card::Counterspell, Card::Island]
        );
        assert_eq!(after.opponent.graveyard, vec![Card::Counterspell]);
        assert_eq!(after.you.graveyard, vec![Card::Thoughtseize]);
        // Duress can't take the creature.
        let state = versus(PlayerState::default(), opponent_hand(hand));
        let asked = resolve(state, Card::Duress);
        assert_eq!(asked.you.life, 20);
        assert_eq!(
            legal_actions(&asked),
            vec![Action::Choose(Choice::Discard(vec![Card::Counterspell]))]
        );
        // Nothing to take: no choice, the spell is just binned.
        let state = versus(
            PlayerState::default(),
            opponent_hand(vec![Card::Island, Card::ThassasOracle]),
        );
        let after = resolve(state, Card::Duress);
        assert_eq!(after.pending, None);
        assert_eq!(after.you.graveyard, vec![Card::Duress]);
        // Thoughtseize at 2 life is fatal.
        let state = versus(
            PlayerState {
                life: 2,
                ..PlayerState::default()
            },
            opponent_hand(vec![Card::Counterspell]),
        );
        let after = resolve(state, Card::Thoughtseize);
        assert_eq!(after.outcome, Some(Outcome::Loss(LossReason::LifeZero)));
    }

    #[test]
    fn evoked_grief_is_sacrificed_but_still_strips_a_card() {
        let mut state = versus(
            PlayerState::default(),
            opponent_hand(vec![Card::ForceOfWill, Card::Island]),
        );
        state.stack.push(StackItem::Spell {
            card: Card::Grief,
            controller: Player::You,
            target: None,
            mode: SpellMode::Evoked,
        });
        let entered = pass_twice(&state);
        assert!(entered.you.battlefield.is_empty());
        assert_eq!(entered.you.graveyard, vec![Card::Grief]);
        assert!(matches!(
            entered.stack.as_slice(),
            [StackItem::Trigger {
                kind: TriggerKind::GriefEnters,
                ..
            }]
        ));
        let asked = pass_twice(&entered);
        assert_eq!(
            legal_actions(&asked),
            vec![Action::Choose(Choice::Discard(vec![Card::ForceOfWill]))]
        );
        let after = apply(
            &asked,
            &Action::Choose(Choice::Discard(vec![Card::ForceOfWill])),
        )
        .unwrap();
        assert_eq!(after.opponent.hand, vec![Card::Island]);
        assert_eq!(after.opponent.graveyard, vec![Card::ForceOfWill]);
        // Hardcast, Grief stays.
        let mut state = versus(PlayerState::default(), opponent_hand(vec![Card::Island]));
        state.stack.push(your_spell(Card::Grief));
        let entered = pass_twice(&state);
        assert_eq!(
            entered.you.battlefield,
            vec![Permanent::entering(Card::Grief, false)]
        );
        // Nothing nonland in their hand: the trigger does nothing.
        let after = pass_twice(&entered);
        assert_eq!(after.pending, None);
    }

    // ------------------------------------------------------------- stage 5

    fn against(you: PlayerState, hate: &[Card]) -> State {
        versus(
            you,
            PlayerState {
                battlefield: hate.iter().map(|&c| Permanent::untapped(c)).collect(),
                ..PlayerState::default()
            },
        )
    }

    #[test]
    fn hate_data() {
        assert_eq!(Card::Narset.kind(), CardKind::Planeswalker);
        assert_eq!(Card::Narset.starting_loyalty(), 5);
        assert!(Card::Narset.is_blue());
        assert_eq!(Card::OrcishBowmasters.kind(), CardKind::Creature);
        assert_eq!(Card::LeylineOfTheVoid.kind(), CardKind::Enchantment);
        assert!(Card::LeylineOfTheVoid.is_black());
        assert_eq!(Card::GrafdiggersCage.kind(), CardKind::Artifact);
        assert!(Card::GrafdiggersCage.is_colorless());
        assert_eq!(Card::Thalia.kind(), CardKind::Creature);
        assert_eq!(Card::ArchonOfEmeria.kind(), CardKind::Creature);
        assert_eq!(Card::Narset.mana_cost().to_string(), "{1}{U}{U}");
        assert_eq!(Card::OrcishBowmasters.mana_cost().to_string(), "{1}{B}");
        assert_eq!(Card::LeylineOfTheVoid.mana_cost().to_string(), "{2}{B}{B}");
        assert_eq!(Card::GrafdiggersCage.mana_cost().to_string(), "{1}");
        assert_eq!(Card::Thalia.mana_cost().to_string(), "{1}{W}");
        assert_eq!(Card::ArchonOfEmeria.mana_cost().to_string(), "{2}{W}");
        for (card, toughness) in [
            (Card::ThassasOracle, 3),
            (Card::LaboratoryManiac, 2),
            (Card::Grief, 2),
            (Card::DoctorDoom, 4),
            (Card::StreetWraith, 4),
            (Card::OrcishBowmasters, 1),
            (Card::Thalia, 1),
            (Card::ArchonOfEmeria, 3),
            (Card::Island, 0),
        ] {
            assert_eq!(card.toughness(), toughness, "{card}");
        }
    }

    #[test]
    fn narset_caps_your_draws_at_one_per_turn() {
        let state = against(
            PlayerState {
                library: vec![Card::Island, Card::Swamp, Card::Plains],
                ..PlayerState::default()
            },
            &[Card::Narset],
        );
        // Brainstorm draws one, then still asks for two put-backs.
        let after = resolve(state.clone(), Card::Brainstorm);
        assert_eq!(after.you.hand, vec![Card::Island]);
        assert_eq!(after.you.cards_drawn_this_turn, 1);
        assert_eq!(after.you.library.len(), 2);
        // Already drew this turn: nothing at all.
        let mut drew = state.clone();
        drew.you.cards_drawn_this_turn = 1;
        let after = resolve(drew, Card::GitaxianProbe);
        assert!(after.you.hand.is_empty());
        assert_eq!(after.outcome, None);
        // An unordered library doesn't matter when no card is drawn.
        let mut scrambled = state;
        scrambled.you.cards_drawn_this_turn = 1;
        scrambled.you.library_ordered = false;
        let after = resolve(scrambled, Card::GitaxianProbe);
        assert_eq!(after.outcome, None);
    }

    #[test]
    fn a_draw_narset_forbids_is_never_replaced_by_lab_man() {
        let state = against(
            PlayerState {
                battlefield: vec![Permanent::untapped(Card::LaboratoryManiac)],
                cards_drawn_this_turn: 1,
                ..PlayerState::default()
            },
            &[Card::Narset],
        );
        let after = resolve(state, Card::GitaxianProbe);
        assert_eq!(after.outcome, None);
        assert!(!after.you.drew_from_empty);
        // Flow State puts into hand, which is not a draw; Doom's win check
        // still fires with an empty library.
        let state = against(
            PlayerState {
                library: vec![Card::Island],
                cards_drawn_this_turn: 1,
                ..PlayerState::default()
            },
            &[Card::Narset],
        );
        let asked = resolve(state, Card::FlowState);
        assert!(asked.pending.is_some());
        let state = with_ability(
            against(
                PlayerState {
                    battlefield: vec![Permanent::untapped(Card::DoctorDoom)],
                    cards_drawn_this_turn: 1,
                    ..PlayerState::default()
                },
                &[Card::Narset],
            ),
            Card::DoctorDoom,
            AbilityKind::DoomDraw,
        );
        assert_eq!(pass_twice(&state).outcome, Some(Outcome::Win));
        // Pyroblast can destroy her.
        let state = against(PlayerState::default(), &[Card::Narset]);
        assert_eq!(
            Card::Pyroblast.target_options(&state, Player::You),
            Some(vec![Target::Permanent {
                owner: Player::Opponent,
                card: Card::Narset,
            }])
        );
    }

    #[test]
    fn bowmasters_pings_once_per_card_drawn() {
        let state = against(
            PlayerState {
                library: vec![Card::Island, Card::Swamp, Card::Plains],
                battlefield: vec![Permanent::untapped(Card::LaboratoryManiac)],
                ..PlayerState::default()
            },
            &[Card::OrcishBowmasters],
        );
        let asked = resolve(state, Card::Brainstorm);
        // Brainstorm's put-back comes first; the three pings wait on the stack.
        assert_eq!(
            asked.pending.map(|p| p.kind),
            Some(ChoiceKind::BrainstormPutBack)
        );
        assert_eq!(asked.stack.len(), 3);
        assert!(asked.stack.iter().all(|item| matches!(
            item,
            StackItem::Trigger {
                kind: TriggerKind::BowmastersPing,
                controller: Player::Opponent,
                ..
            }
        )));
        let after = apply(
            &asked,
            &Action::Choose(Choice::BrainstormPutBack([Card::Island, Card::Swamp])),
        )
        .unwrap();
        // The opponent decides each ping: you, or Lab Man.
        let ping = pass_twice(&after);
        let pending = ping.pending.expect("the opponent aims the ping");
        assert_eq!(pending.player, Player::Opponent);
        assert_eq!(
            legal_actions(&ping),
            vec![
                Action::Choose(Choice::Ping(Target::Player(Player::You))),
                Action::Choose(Choice::Ping(your_permanent(Card::LaboratoryManiac))),
            ]
        );
        let face = apply(
            &ping,
            &Action::Choose(Choice::Ping(Target::Player(Player::You))),
        )
        .unwrap();
        assert_eq!(face.you.life, 19);
        let hit = apply(
            &ping,
            &Action::Choose(Choice::Ping(your_permanent(Card::LaboratoryManiac))),
        )
        .unwrap();
        assert_eq!(hit.you.battlefield[0].damage, 1);
        assert!(hit.you.battlefield[0].to_string().contains("1 damage"));
        // The second ping kills Lab Man.
        let hit = apply(
            &pass_twice(&hit),
            &Action::Choose(Choice::Ping(your_permanent(Card::LaboratoryManiac))),
        )
        .unwrap();
        assert!(hit.you.battlefield.is_empty());
        assert_eq!(
            hit.you.graveyard,
            vec![Card::LaboratoryManiac, Card::Brainstorm]
        );
    }

    #[test]
    fn bowmasters_ping_at_one_life_kills_you_and_a_replaced_draw_never_pings() {
        let state = against(
            PlayerState {
                life: 1,
                library: vec![Card::Island],
                ..PlayerState::default()
            },
            &[Card::OrcishBowmasters],
        );
        let ping = pass_twice(&resolve(state, Card::GitaxianProbe));
        assert_eq!(
            legal_actions(&ping),
            vec![Action::Choose(Choice::Ping(Target::Player(Player::You)))]
        );
        let dead = apply(
            &ping,
            &Action::Choose(Choice::Ping(Target::Player(Player::You))),
        )
        .unwrap();
        assert_eq!(dead.outcome, Some(Outcome::Loss(LossReason::LifeZero)));
        // Lab Man's replacement wins without any trigger.
        let state = against(
            PlayerState {
                battlefield: vec![Permanent::untapped(Card::LaboratoryManiac)],
                ..PlayerState::default()
            },
            &[Card::OrcishBowmasters],
        );
        let won = resolve(state, Card::GitaxianProbe);
        assert_eq!(won.outcome, Some(Outcome::Win));
        assert!(won.stack.is_empty());
    }

    #[test]
    fn a_creature_killed_under_leyline_is_exiled() {
        let mut state = against(
            PlayerState {
                battlefield: vec![Permanent::untapped(Card::LaboratoryManiac)],
                ..PlayerState::default()
            },
            &[Card::OrcishBowmasters, Card::LeylineOfTheVoid],
        );
        state.you.battlefield[0].damage = 1;
        state.stack.push(StackItem::Trigger {
            source: Card::OrcishBowmasters,
            kind: TriggerKind::BowmastersPing,
            controller: Player::Opponent,
        });
        let ping = pass_twice(&state);
        let dead = apply(
            &ping,
            &Action::Choose(Choice::Ping(your_permanent(Card::LaboratoryManiac))),
        )
        .unwrap();
        assert!(dead.you.graveyard.is_empty());
        assert_eq!(dead.you.exile, vec![Card::LaboratoryManiac]);
    }

    #[test]
    fn leyline_empties_your_graveyard_everywhere() {
        let state = against(
            PlayerState {
                hand: vec![Card::StreetWraith],
                library: vec![Card::Island, Card::Swamp],
                battlefield: vec![Permanent::untapped(Card::LotusPetal)],
                graveyard: vec![Card::Doomsday],
                mana_pool: ManaPool::of(Color::B, 2),
                ..PlayerState::default()
            },
            &[Card::LeylineOfTheVoid],
        );
        // A resolved spell.
        let after = resolve(state.clone(), Card::DarkRitual);
        assert_eq!(after.you.graveyard, vec![Card::Doomsday]);
        assert_eq!(after.you.exile, vec![Card::DarkRitual]);
        // Cabal Ritual stays at three (nothing piles up) and is exiled too;
        // Unearth has no targets.
        let after = resolve(state.clone(), Card::CabalRitual);
        assert_eq!(after.you.mana_pool.get(Color::B), 5);
        assert_eq!(after.you.exile, vec![Card::CabalRitual]);
        assert_eq!(
            Card::Unearth.target_options(&state, Player::You),
            Some(Vec::new())
        );
        // A surveil, a cycled card and a sacrificed Petal.
        let mut sewers = state.clone();
        sewers.pending = Some(PendingChoice {
            player: Player::You,
            kind: ChoiceKind::Surveil,
            bin: None,
            bin_owner: Player::You,
        });
        let after = apply(
            &sewers,
            &Action::Choose(Choice::Surveil { to_graveyard: true }),
        )
        .unwrap();
        assert_eq!(after.you.exile, vec![Card::Island]);
        let after = apply(
            &state,
            &Action::Cycle {
                card: Card::StreetWraith,
                payment: Payment::life(2),
                sacrifice: None,
            },
        )
        .unwrap();
        assert_eq!(after.you.exile, vec![Card::StreetWraith]);
        let after = apply(
            &state,
            &Action::SacrificeForMana {
                source: Card::LotusPetal,
                color: Color::U,
            },
        )
        .unwrap();
        assert_eq!(after.you.exile, vec![Card::LotusPetal]);
        assert_eq!(after.you.graveyard, vec![Card::Doomsday]);
    }

    #[test]
    fn bouncing_leyline_turns_the_graveyard_back_on() {
        let mut state = against(
            PlayerState {
                battlefield: vec![Permanent::untapped(Card::Teferi)],
                library: vec![Card::Island],
                ..PlayerState::default()
            },
            &[Card::LeylineOfTheVoid],
        );
        state.stack.push(StackItem::Ability {
            source: Card::Teferi,
            kind: AbilityKind::TeferiMinusThree {
                target: Some(Target::Permanent {
                    owner: Player::Opponent,
                    card: Card::LeylineOfTheVoid,
                }),
            },
            controller: Player::You,
        });
        let after = pass_twice(&state);
        assert_eq!(after.opponent.hand, vec![Card::LeylineOfTheVoid]);
        assert!(after.opponent.battlefield.is_empty());
        let after = resolve(after, Card::DarkRitual);
        assert_eq!(after.you.graveyard, vec![Card::DarkRitual]);
    }

    #[test]
    fn grafdiggers_cage_keeps_unearths_target_in_the_graveyard() {
        let state = against(
            PlayerState {
                graveyard: vec![Card::Doomsday, Card::ThassasOracle],
                library: vec![Card::Island],
                ..PlayerState::default()
            },
            &[Card::GrafdiggersCage],
        );
        // Still a legal target, but nothing enters.
        assert_eq!(
            Card::Unearth.target_options(&state, Player::You),
            Some(vec![your_graveyard(Card::ThassasOracle)])
        );
        let after = resolve_targeting(
            state,
            Card::Unearth,
            Some(your_graveyard(Card::ThassasOracle)),
        );
        assert!(after.you.battlefield.is_empty());
        assert!(after.stack.is_empty());
        assert_eq!(
            after.you.graveyard,
            vec![Card::ThassasOracle, Card::Doomsday, Card::Unearth]
        );
    }
}
