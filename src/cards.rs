//! The card pool and every card-specific behavior, as `match` arms.
//!
//! Only cards from `CARDS.md` belong here. Stages 1 and 2 cover Thassa's
//! Oracle, Jace, Doctor Doom, Laboratory Maniac, Doomsday, the blue cantrips,
//! Probe, Street Wraith, Edge of Autumn, the fast mana, and the basic, dual,
//! fetch and surveil lands.

use std::fmt;

use crate::engine::{Choice, EngineError, ordered_selections};
use crate::mana::{Color, ManaCost};
use crate::state::{ChoiceKind, LossReason, PendingChoice, Player, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Card {
    // Win conditions
    ThassasOracle,
    JaceWielderOfMysteries,
    DoctorDoom,
    LaboratoryManiac,
    // Doomsday
    Doomsday,
    // Draw and selection
    Brainstorm,
    Ponder,
    Preordain,
    Consider,
    AncestralRecall,
    GitaxianProbe,
    StreetWraith,
    EdgeOfAutumn,
    // Mana
    DarkRitual,
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
}

/// A loyalty ability a planeswalker can activate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoyaltyAbility {
    JacePlusOne { target: Player },
}

impl LoyaltyAbility {
    /// Loyalty added (or, negative, removed) as the cost.
    pub fn loyalty_change(self) -> i8 {
        match self {
            LoyaltyAbility::JacePlusOne { .. } => 1,
        }
    }

    pub fn ability(self) -> AbilityKind {
        match self {
            LoyaltyAbility::JacePlusOne { target } => AbilityKind::JacePlusOne { target },
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
            Card::Brainstorm => "Brainstorm",
            Card::Ponder => "Ponder",
            Card::Preordain => "Preordain",
            Card::Consider => "Consider",
            Card::AncestralRecall => "Ancestral Recall",
            Card::GitaxianProbe => "Gitaxian Probe",
            Card::StreetWraith => "Street Wraith",
            Card::EdgeOfAutumn => "Edge of Autumn",
            Card::DarkRitual => "Dark Ritual",
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
            | Card::StreetWraith => CardKind::Creature,
            Card::JaceWielderOfMysteries => CardKind::Planeswalker,
            Card::Doomsday
            | Card::Ponder
            | Card::Preordain
            | Card::GitaxianProbe
            | Card::EdgeOfAutumn => CardKind::Sorcery,
            Card::Brainstorm | Card::Consider | Card::AncestralRecall | Card::DarkRitual => {
                CardKind::Instant
            }
            Card::LionsEyeDiamond
            | Card::BlackLotus
            | Card::LotusPetal
            | Card::MoxSapphire
            | Card::MoxJet => CardKind::Artifact,
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
            Card::Brainstorm
            | Card::Ponder
            | Card::Preordain
            | Card::Consider
            | Card::AncestralRecall => ManaCost::colored(Color::U, 1),
            Card::GitaxianProbe => ManaCost::phyrexian(Color::U, 1),
            Card::StreetWraith => ManaCost::colored(Color::B, 2).with_generic(3),
            Card::EdgeOfAutumn => ManaCost::colored(Color::G, 1).with_generic(1),
            Card::DarkRitual => ManaCost::colored(Color::B, 1),
            _ => ManaCost::FREE,
        }
    }

    /// Whether the engine models casting this card at all. Street Wraith is
    /// cycling-only per CARDS.md.
    pub fn castable(self) -> bool {
        self != Card::StreetWraith
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
            _ => None,
        }
    }

    /// Loyalty a planeswalker enters with (0 for everything else).
    pub fn starting_loyalty(self) -> u8 {
        match self {
            Card::JaceWielderOfMysteries => 4,
            _ => 0,
        }
    }

    pub fn loyalty_abilities(self) -> Vec<LoyaltyAbility> {
        match self {
            Card::JaceWielderOfMysteries => vec![
                LoyaltyAbility::JacePlusOne {
                    target: Player::You,
                },
                LoyaltyAbility::JacePlusOne {
                    target: Player::Opponent,
                },
            ],
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
        }
    }
}

/// Resolves an instant or sorcery. Cards that need a decision set
/// `state.pending` and finish in [`apply_choice`]; the others go straight to
/// the graveyard.
pub(crate) fn resolve_spell(
    state: &mut State,
    card: Card,
    controller: Player,
) -> Result<(), EngineError> {
    match card {
        Card::Brainstorm => {
            state.draw(controller, 3);
            if state.outcome.is_none() {
                return pause(state, controller, Some(card), ChoiceKind::BrainstormPutBack);
            }
        }
        Card::Ponder => {
            if look_at_top(state, controller, 3) > 0 {
                return pause(state, controller, Some(card), ChoiceKind::PonderOrder);
            }
            state.draw(controller, 1);
        }
        Card::Preordain => {
            if look_at_top(state, controller, 2) > 0 {
                return pause(state, controller, Some(card), ChoiceKind::PreordainScry);
            }
            state.draw(controller, 1);
        }
        Card::Consider => {
            if look_at_top(state, controller, 1) > 0 {
                return pause(state, controller, Some(card), ChoiceKind::ConsiderSurveil);
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
                    Some(card),
                    ChoiceKind::SearchLand(LandFilter::Basic),
                );
            }
        }
        Card::DarkRitual => state.player_mut(controller).mana_pool.add_mana(Color::B, 3),
        _ => return Err(EngineError::Internal("not an instant or sorcery")),
    }
    state.player_mut(controller).graveyard.push(card);
    Ok(())
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

fn pause(
    state: &mut State,
    player: Player,
    bin: Option<Card>,
    kind: ChoiceKind,
) -> Result<(), EngineError> {
    if state.pending.is_some() {
        return Err(EngineError::Internal("two choices pending at once"));
    }
    state.pending = Some(PendingChoice { player, kind, bin });
    Ok(())
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
            state.player_mut(target).mill(k);
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
    }
    out
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
                ps.mill(1);
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
        _ => return Err(illegal()),
    }
    if let Some(card) = pending.bin {
        state.player_mut(player).graveyard.push(card);
    }
    Ok(())
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
    use crate::state::{Outcome, Permanent, PlayerState, StackItem};

    /// Resolves the top of the stack with two passes.
    fn pass_twice(state: &State) -> State {
        apply(&apply(state, &Action::Pass).unwrap(), &Action::Pass).unwrap()
    }

    /// Puts `card` on the stack as your spell and resolves it with two passes.
    fn resolve(mut state: State, card: Card) -> State {
        state.stack.push(StackItem::Spell {
            card,
            controller: Player::You,
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
}
