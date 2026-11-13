//! The card pool and every card-specific behavior, as `match` arms.
//!
//! Only cards from `CARDS.md` belong here. Stage 1 covers Thassa's Oracle,
//! Doomsday, the blue cantrips, the fast mana and the basic and dual lands.

use std::fmt;

use crate::engine::{Choice, EngineError, ordered_selections};
use crate::mana::{Color, ManaCost};
use crate::state::{ChoiceKind, LossReason, Outcome, PendingChoice, Player, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Card {
    // Win conditions
    ThassasOracle,
    // Doomsday
    Doomsday,
    // Draw and selection
    Brainstorm,
    Ponder,
    Preordain,
    Consider,
    AncestralRecall,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CardKind {
    Instant,
    Sorcery,
    Creature,
    Artifact,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TriggerKind {
    /// Thassa's Oracle's enter trigger.
    OracleEnters,
}

impl Card {
    pub fn name(self) -> &'static str {
        match self {
            Card::ThassasOracle => "Thassa's Oracle",
            Card::Doomsday => "Doomsday",
            Card::Brainstorm => "Brainstorm",
            Card::Ponder => "Ponder",
            Card::Preordain => "Preordain",
            Card::Consider => "Consider",
            Card::AncestralRecall => "Ancestral Recall",
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
        }
    }

    pub fn kind(self) -> CardKind {
        match self {
            Card::ThassasOracle => CardKind::Creature,
            Card::Doomsday | Card::Ponder | Card::Preordain => CardKind::Sorcery,
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
            | Card::Taiga => CardKind::Land,
        }
    }

    pub fn mana_cost(self) -> ManaCost {
        match self {
            Card::ThassasOracle => ManaCost::colored(Color::U, 2),
            Card::Doomsday => ManaCost::colored(Color::B, 3),
            Card::Brainstorm
            | Card::Ponder
            | Card::Preordain
            | Card::Consider
            | Card::AncestralRecall => ManaCost::colored(Color::U, 1),
            Card::DarkRitual => ManaCost::colored(Color::B, 1),
            _ => ManaCost::FREE,
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
            Card::UndergroundSea => &[Island, Swamp],
            Card::Tundra => &[Plains, Island],
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

    pub fn mana_ability(self) -> Option<ManaAbility> {
        match self {
            Card::LionsEyeDiamond => Some(ManaAbility::Led),
            Card::BlackLotus => Some(ManaAbility::Sacrifice(3)),
            Card::LotusPetal => Some(ManaAbility::Sacrifice(1)),
            Card::MoxSapphire | Card::MoxJet => Some(ManaAbility::Tap),
            _ if self.kind() == CardKind::Land => Some(ManaAbility::Tap),
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
            _ => None,
        }
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
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
                return pause(state, controller, card, ChoiceKind::BrainstormPutBack);
            }
        }
        Card::Ponder => {
            if look_at_top(state, controller, 3) > 0 {
                return pause(state, controller, card, ChoiceKind::PonderOrder);
            }
            state.draw(controller, 1);
        }
        Card::Preordain => {
            if look_at_top(state, controller, 2) > 0 {
                return pause(state, controller, card, ChoiceKind::PreordainScry);
            }
            state.draw(controller, 1);
        }
        Card::Consider => {
            if look_at_top(state, controller, 1) > 0 {
                return pause(state, controller, card, ChoiceKind::ConsiderSurveil);
            }
            state.draw(controller, 1);
        }
        Card::AncestralRecall => state.draw(controller, 3),
        Card::DarkRitual => state
            .player_mut(controller)
            .mana_pool
            .add_mana(Color::B, 3),
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
    card: Card,
    kind: ChoiceKind,
) -> Result<(), EngineError> {
    if state.pending.is_some() {
        return Err(EngineError::Internal("two choices pending at once"));
    }
    state.pending = Some(PendingChoice { player, card, kind });
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
                if controller == Player::You {
                    state.outcome = Some(Outcome::Win);
                }
            } else if x > 0 {
                // The looked-at cards go to the bottom in a random order.
                ps.library_ordered = false;
            }
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
        ChoiceKind::ConsiderSurveil => {
            out.push(Choice::ConsiderSurveil {
                to_graveyard: false,
            });
            out.push(Choice::ConsiderSurveil { to_graveyard: true });
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
        (ChoiceKind::ConsiderSurveil, Choice::ConsiderSurveil { to_graveyard }) => {
            let ps = state.player_mut(player);
            if ps.library.is_empty() {
                return Err(illegal());
            }
            if *to_graveyard {
                let card = ps.library.remove(0);
                ps.graveyard.push(card);
            }
            state.draw(player, 1);
        }
        _ => return Err(illegal()),
    }
    state.player_mut(player).graveyard.push(pending.card);
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
    use crate::state::{Permanent, PlayerState, StackItem};

    /// Puts `card` on the stack as your spell and resolves it with two passes.
    fn resolve(mut state: State, card: Card) -> State {
        state.stack.push(StackItem::Spell {
            card,
            controller: Player::You,
        });
        let state = apply(&state, &Action::Pass).unwrap();
        apply(&state, &Action::Pass).unwrap()
    }

    #[test]
    fn card_data() {
        assert_eq!(Card::ThassasOracle.blue_devotion(), 2);
        assert_eq!(Card::MoxSapphire.blue_devotion(), 0);
        assert_eq!(Card::ThassasOracle.kind(), CardKind::Creature);
        assert_eq!(Card::Brainstorm.kind(), CardKind::Instant);
        assert_eq!(Card::Ponder.kind(), CardKind::Sorcery);
        assert_eq!(Card::LionsEyeDiamond.kind(), CardKind::Artifact);
        assert_eq!(Card::UndergroundSea.kind(), CardKind::Land);
        assert_eq!(Card::Doomsday.mana_cost().mana_value(), 3);
        assert_eq!(Card::LionsEyeDiamond.mana_cost(), ManaCost::FREE);
    }

    #[test]
    fn duals_make_both_colors_from_subtypes() {
        assert_eq!(Card::UndergroundSea.mana_colors(), vec![Color::U, Color::B]);
        assert_eq!(Card::Taiga.mana_colors(), vec![Color::R, Color::G]);
        assert_eq!(Card::Island.mana_colors(), vec![Color::U]);
        assert_eq!(Card::MoxJet.mana_colors(), vec![Color::B]);
        assert_eq!(Card::ThassasOracle.mana_ability(), None);
    }

    #[test]
    fn ancestral_draws_three() {
        let state = main_phase(PlayerState {
            library: vec![Card::Island, Card::Swamp, Card::Plains, Card::Forest],
            ..PlayerState::default()
        });
        let state = resolve(state, Card::AncestralRecall);
        assert_eq!(state.you.hand, vec![Card::Plains, Card::Island, Card::Swamp]);
        assert_eq!(state.you.library, vec![Card::Forest]);
        assert_eq!(state.you.graveyard, vec![Card::AncestralRecall]);
        assert_eq!(state.you.cards_drawn_this_turn, 3);
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
        assert_eq!(next.you.library, vec![Card::Island, Card::Swamp, Card::Forest]);
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
            &Action::Choose(Choice::ConsiderSurveil { to_graveyard: true }),
        )
        .unwrap();
        assert_eq!(binned.you.hand, vec![Card::ThassasOracle]);
        assert!(binned.you.library.is_empty());
        assert_eq!(binned.you.graveyard, vec![Card::Consider, Card::Island]);
        let kept = apply(
            &state,
            &Action::Choose(Choice::ConsiderSurveil {
                to_graveyard: false,
            }),
        )
        .unwrap();
        assert_eq!(kept.you.hand, vec![Card::Island]);
    }

    #[test]
    fn cantrips_on_an_empty_library_lose() {
        for card in [Card::Ponder, Card::Preordain, Card::Consider] {
            let state = resolve(main_phase(PlayerState::default()), card);
            assert_eq!(
                state.outcome,
                Some(Outcome::Loss(LossReason::DrewFromEmptyLibrary)),
                "{card}"
            );
        }
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
                payment: ManaPool::of(Color::U, 2),
            },
        )
        .unwrap();
        let resolved = apply(&apply(&cast, &Action::Pass).unwrap(), &Action::Pass).unwrap();
        // Oracle is on the battlefield and its trigger is on the stack.
        assert_eq!(
            resolved.you.battlefield,
            vec![Permanent {
                card: Card::ThassasOracle,
                tapped: false,
                summoning_sick: true,
            }]
        );
        assert!(matches!(
            resolved.stack.as_slice(),
            [StackItem::Trigger {
                kind: TriggerKind::OracleEnters,
                ..
            }]
        ));
        assert_eq!(resolved.outcome, None);
        let won = apply(&apply(&resolved, &Action::Pass).unwrap(), &Action::Pass).unwrap();
        assert_eq!(won.outcome, Some(Outcome::Win));
    }

    #[test]
    fn oracle_wins_on_empty_library() {
        let state = main_phase(PlayerState {
            battlefield: vec![Permanent::untapped(Card::ThassasOracle)],
            ..PlayerState::default()
        });
        let mut state = state;
        state.stack.push(StackItem::Trigger {
            source: Card::ThassasOracle,
            kind: TriggerKind::OracleEnters,
            controller: Player::You,
        });
        let won = apply(&apply(&state, &Action::Pass).unwrap(), &Action::Pass).unwrap();
        assert_eq!(won.outcome, Some(Outcome::Win));
    }

    #[test]
    fn oracle_one_short_scrambles_the_library() {
        let mut state = main_phase(PlayerState {
            battlefield: vec![Permanent::untapped(Card::ThassasOracle)],
            library: vec![Card::Island, Card::Swamp, Card::Plains],
            ..PlayerState::default()
        });
        state.stack.push(StackItem::Trigger {
            source: Card::ThassasOracle,
            kind: TriggerKind::OracleEnters,
            controller: Player::You,
        });
        let after = apply(&apply(&state, &Action::Pass).unwrap(), &Action::Pass).unwrap();
        assert_eq!(after.outcome, None);
        assert!(!after.you.library_ordered);
        assert_eq!(after.you.library.len(), 3);
    }
}
