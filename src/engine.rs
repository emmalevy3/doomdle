//! Actions, legality and the state transition function.
//!
//! [`legal_actions`] is the only place that decides what a player may do.
//! [`apply`] is pure: it returns a new state and never mutates its input.

use std::fmt;

use crate::cards::{self, Card, CardKind, ManaAbility};
use crate::mana::{Color, ManaPool};
use crate::state::{LossReason, Permanent, Phase, Player, StackItem, State};

/// A player's answer to a [`crate::state::PendingChoice`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Choice {
    /// Two cards from hand to put on top of the library: `[top, second]`.
    BrainstormPutBack([Card; 2]),
    /// The new order of the looked-at cards, top first.
    PonderOrder(Vec<Card>),
    /// Cards kept on top (in order) and cards sent to the bottom (in order).
    PreordainScry { top: Vec<Card>, bottom: Vec<Card> },
    ConsiderSurveil { to_graveyard: bool },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    /// Doomsday's pile, top first. Only legal in [`Phase::ChoosingPile`].
    ChoosePile(Vec<Card>),
    Pass,
    /// Cast `card` from hand, paying exactly `payment`.
    CastSpell { card: Card, payment: ManaPool },
    PlayLand(Card),
    /// Tap an untapped permanent with this card name for one mana.
    TapForMana { source: Card, color: Color },
    /// Black Lotus or Lotus Petal.
    SacrificeForMana { source: Card, color: Color },
    /// Lion's Eye Diamond: discard your hand, add three of `color`.
    ActivateLed { color: Color },
    Choose(Choice),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineError {
    GameOver,
    IllegalAction(Action),
    /// The engine reached a situation its rules don't cover: a bug, not a
    /// player mistake.
    Internal(&'static str),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::GameOver => write!(f, "the game is already over"),
            EngineError::IllegalAction(action) => write!(f, "illegal action: {action}"),
            EngineError::Internal(msg) => write!(f, "internal engine error: {msg}"),
        }
    }
}

impl std::error::Error for EngineError {}

/// Everything the player who must act next may do.
pub fn legal_actions(state: &State) -> Vec<Action> {
    if state.outcome.is_some() {
        return Vec::new();
    }
    if state.phase == Phase::ChoosingPile {
        let mut candidates = state.you.library.clone();
        candidates.extend(&state.you.graveyard);
        let k = candidates.len().min(5);
        return ordered_selections(&candidates, k)
            .into_iter()
            .map(Action::ChoosePile)
            .collect();
    }
    if let Some(pending) = &state.pending {
        return cards::choice_options(state, pending)
            .into_iter()
            .map(Action::Choose)
            .collect();
    }
    match state.priority {
        Player::You => your_actions(state),
        // Stage 4 adds the opponent's responses.
        Player::Opponent => vec![Action::Pass],
    }
}

/// Ordered so that passing and casting come before mana abilities: the solver
/// tries actions in this order, and resolving or casting what you already can
/// pay for beats floating mana you may never use.
fn your_actions(state: &State) -> Vec<Action> {
    let you = &state.you;
    let sorcery_speed = state.stack.is_empty();
    let mut out = vec![Action::Pass];

    for card in distinct(&you.hand) {
        match card.kind() {
            CardKind::Land => {
                if sorcery_speed && you.land_drop_available {
                    out.push(Action::PlayLand(card));
                }
            }
            CardKind::Instant => push_casts(&mut out, you.mana_pool, card),
            CardKind::Sorcery | CardKind::Creature | CardKind::Artifact => {
                if sorcery_speed {
                    push_casts(&mut out, you.mana_pool, card);
                }
            }
        }
    }

    for perm in distinct(&you.battlefield) {
        match perm.card.mana_ability() {
            Some(ManaAbility::Tap) if !perm.tapped => {
                for color in perm.card.mana_colors() {
                    out.push(Action::TapForMana {
                        source: perm.card,
                        color,
                    });
                }
            }
            Some(ManaAbility::Sacrifice(_)) if !perm.tapped => {
                for color in Color::ALL {
                    out.push(Action::SacrificeForMana {
                        source: perm.card,
                        color,
                    });
                }
            }
            // "Activate only as an instant": whenever you hold priority.
            Some(ManaAbility::Led) => {
                for color in Color::ALL {
                    out.push(Action::ActivateLed { color });
                }
            }
            _ => {}
        }
    }

    out
}

fn push_casts(out: &mut Vec<Action>, pool: ManaPool, card: Card) {
    for payment in pool.payments_for(&card.mana_cost()) {
        out.push(Action::CastSpell { card, payment });
    }
}

/// Sorted, deduplicated copy: identical cards or permanents give one action.
fn distinct<T: Ord + Clone>(items: &[T]) -> Vec<T> {
    let mut v = items.to_vec();
    v.sort();
    v.dedup();
    v
}

/// Every distinct ordered selection of `k` items from a multiset.
pub(crate) fn ordered_selections(items: &[Card], k: usize) -> Vec<Vec<Card>> {
    let mut pool = items.to_vec();
    pool.sort();
    let mut out = Vec::new();
    let mut acc = Vec::with_capacity(k);
    select(&mut pool, k, &mut acc, &mut out);
    out
}

fn select(pool: &mut Vec<Card>, k: usize, acc: &mut Vec<Card>, out: &mut Vec<Vec<Card>>) {
    if acc.len() == k {
        out.push(acc.clone());
        return;
    }
    let mut i = 0;
    while i < pool.len() {
        // `pool` is sorted, so equal cards are adjacent: skip repeats.
        if i > 0 && pool[i] == pool[i - 1] {
            i += 1;
            continue;
        }
        let card = pool.remove(i);
        acc.push(card);
        select(pool, k, acc, out);
        acc.pop();
        pool.insert(i, card);
        i += 1;
    }
}

/// Applies `action` and returns the resulting state.
pub fn apply(state: &State, action: &Action) -> Result<State, EngineError> {
    if state.outcome.is_some() {
        return Err(EngineError::GameOver);
    }
    let mut next = state.clone();
    if let Action::ChoosePile(pile) = action {
        choose_pile(&mut next, pile)?;
    } else {
        if state.phase != Phase::Main || !legal_actions(state).contains(action) {
            return Err(EngineError::IllegalAction(action.clone()));
        }
        let actor = state
            .actor()
            .ok_or(EngineError::Internal("no actor on a live state"))?;
        match action {
            Action::ChoosePile(_) => unreachable!("handled above"),
            Action::Pass => pass(&mut next)?,
            Action::CastSpell { card, payment } => cast(&mut next, actor, *card, payment)?,
            Action::PlayLand(card) => play_land(&mut next, actor, *card)?,
            Action::TapForMana { source, color } => tap_for_mana(&mut next, actor, *source, *color)?,
            Action::SacrificeForMana { source, color } => {
                sacrifice_for_mana(&mut next, actor, *source, *color)?
            }
            Action::ActivateLed { color } => activate_led(&mut next, actor, *color)?,
            Action::Choose(choice) => choose(&mut next, choice)?,
        }
    }
    next.check_state_based();
    next.normalize();
    Ok(next)
}

/// Doomsday resolves: the chosen cards become the library, everything else
/// in library and graveyard is exiled, Doomsday goes to the graveyard, and you
/// lose half your life rounded up.
fn choose_pile(state: &mut State, pile: &[Card]) -> Result<(), EngineError> {
    let illegal = || EngineError::IllegalAction(Action::ChoosePile(pile.to_vec()));
    if state.phase != Phase::ChoosingPile {
        return Err(illegal());
    }
    let you = &mut state.you;
    let mut rest = you.library.clone();
    rest.extend(&you.graveyard);
    if pile.len() != rest.len().min(5) {
        return Err(illegal());
    }
    for card in pile {
        let pos = rest.iter().position(|c| c == card).ok_or_else(illegal)?;
        rest.remove(pos);
    }
    you.library = pile.to_vec();
    you.library_ordered = true;
    you.exile.extend(rest);
    you.graveyard = vec![Card::Doomsday];
    you.life -= (you.life.max(0) + 1) / 2;

    state.phase = Phase::Main;
    state.priority = Player::You;
    state.passed_in_succession = false;
    Ok(())
}

fn pass(state: &mut State) -> Result<(), EngineError> {
    if state.passed_in_succession {
        match state.stack.pop() {
            Some(item) => resolve(state, item)?,
            None => state.lose(LossReason::TurnEnded),
        }
        // After a resolution the active player gets priority.
        state.priority = Player::You;
        state.passed_in_succession = false;
    } else {
        state.passed_in_succession = true;
        state.priority = state.priority.other();
    }
    Ok(())
}

fn resolve(state: &mut State, item: StackItem) -> Result<(), EngineError> {
    match item {
        StackItem::Spell { card, controller } => match card.kind() {
            CardKind::Creature | CardKind::Artifact => {
                state.player_mut(controller).battlefield.push(Permanent {
                    card,
                    tapped: false,
                    summoning_sick: card.kind() == CardKind::Creature,
                });
                if let Some(kind) = card.etb_trigger() {
                    state.stack.push(StackItem::Trigger {
                        source: card,
                        kind,
                        controller,
                    });
                }
                Ok(())
            }
            CardKind::Instant | CardKind::Sorcery => cards::resolve_spell(state, card, controller),
            CardKind::Land => Err(EngineError::Internal("a land can't be on the stack")),
        },
        StackItem::Trigger {
            kind, controller, ..
        } => cards::resolve_trigger(state, kind, controller),
    }
}

fn cast(
    state: &mut State,
    player: Player,
    card: Card,
    payment: &ManaPool,
) -> Result<(), EngineError> {
    let illegal = || {
        EngineError::IllegalAction(Action::CastSpell {
            card,
            payment: *payment,
        })
    };
    let ps = state.player_mut(player);
    if !ps.mana_pool.payments_for(&card.mana_cost()).contains(payment) {
        return Err(illegal());
    }
    ps.mana_pool = ps.mana_pool.minus(payment).ok_or_else(illegal)?;
    ps.remove_from_hand(card).ok_or_else(illegal)?;
    ps.spells_cast_this_turn += 1;
    state.stack.push(StackItem::Spell {
        card,
        controller: player,
    });
    acted(state, player);
    Ok(())
}

fn play_land(state: &mut State, player: Player, card: Card) -> Result<(), EngineError> {
    let ps = state.player_mut(player);
    if !ps.land_drop_available || card.kind() != CardKind::Land {
        return Err(EngineError::IllegalAction(Action::PlayLand(card)));
    }
    ps.remove_from_hand(card)
        .ok_or(EngineError::IllegalAction(Action::PlayLand(card)))?;
    ps.land_drop_available = false;
    ps.battlefield.push(Permanent::untapped(card));
    acted(state, player);
    Ok(())
}

/// Index of the first permanent named `source` that passes `pred`.
fn find_permanent(
    state: &State,
    player: Player,
    source: Card,
    pred: impl Fn(&Permanent) -> bool,
) -> Option<usize> {
    state
        .player(player)
        .battlefield
        .iter()
        .position(|p| p.card == source && pred(p))
}

fn tap_for_mana(
    state: &mut State,
    player: Player,
    source: Card,
    color: Color,
) -> Result<(), EngineError> {
    let illegal = || EngineError::IllegalAction(Action::TapForMana { source, color });
    if source.mana_ability() != Some(ManaAbility::Tap) || !source.mana_colors().contains(&color) {
        return Err(illegal());
    }
    let idx = find_permanent(state, player, source, |p| !p.tapped).ok_or_else(illegal)?;
    let ps = state.player_mut(player);
    ps.battlefield[idx].tapped = true;
    ps.mana_pool.add_mana(color, 1);
    acted(state, player);
    Ok(())
}

fn sacrifice_for_mana(
    state: &mut State,
    player: Player,
    source: Card,
    color: Color,
) -> Result<(), EngineError> {
    let illegal = || EngineError::IllegalAction(Action::SacrificeForMana { source, color });
    let Some(ManaAbility::Sacrifice(amount)) = source.mana_ability() else {
        return Err(illegal());
    };
    let idx = find_permanent(state, player, source, |p| !p.tapped).ok_or_else(illegal)?;
    let ps = state.player_mut(player);
    ps.battlefield.remove(idx);
    ps.graveyard.push(source);
    ps.mana_pool.add_mana(color, amount);
    acted(state, player);
    Ok(())
}

fn activate_led(state: &mut State, player: Player, color: Color) -> Result<(), EngineError> {
    let source = Card::LionsEyeDiamond;
    let idx = find_permanent(state, player, source, |_| true)
        .ok_or(EngineError::IllegalAction(Action::ActivateLed { color }))?;
    let ps = state.player_mut(player);
    ps.battlefield.remove(idx);
    let discarded = std::mem::take(&mut ps.hand);
    ps.graveyard.extend(discarded);
    ps.graveyard.push(source);
    ps.mana_pool.add_mana(color, 3);
    acted(state, player);
    Ok(())
}

fn choose(state: &mut State, choice: &Choice) -> Result<(), EngineError> {
    let pending = state
        .pending
        .take()
        .ok_or_else(|| EngineError::IllegalAction(Action::Choose(choice.clone())))?;
    cards::apply_choice(state, pending, choice)?;
    // The resolution that paused is now complete; the active player already
    // holds priority, nobody has passed since.
    state.passed_in_succession = false;
    Ok(())
}

/// Bookkeeping after a player takes a non-pass action: they keep priority and
/// any run of passes is broken.
fn acted(state: &mut State, player: Player) {
    state.priority = player;
    state.passed_in_succession = false;
}

impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Choice::BrainstormPutBack([top, second]) => {
                write!(f, "Brainstorm: put back {top} on top, then {second}")
            }
            Choice::PonderOrder(order) => write!(f, "Ponder: new order {}", list(order)),
            Choice::PreordainScry { top, bottom } => {
                write!(
                    f,
                    "Preordain: keep {} on top, {} to the bottom",
                    list(top),
                    list(bottom)
                )
            }
            Choice::ConsiderSurveil { to_graveyard } => {
                if *to_graveyard {
                    write!(f, "Consider: top card to graveyard")
                } else {
                    write!(f, "Consider: keep top card")
                }
            }
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::ChoosePile(pile) => write!(f, "Doomsday pile (top first): {}", list(pile)),
            Action::Pass => write!(f, "Pass priority"),
            Action::CastSpell { card, payment } => {
                if payment.is_empty() {
                    write!(f, "Cast {card} for free")
                } else {
                    write!(f, "Cast {card} paying {payment}")
                }
            }
            Action::PlayLand(card) => write!(f, "Play {card}"),
            Action::TapForMana { source, color } => write!(f, "Tap {source} for {color}"),
            Action::SacrificeForMana { source, color } => {
                write!(f, "Sacrifice {source} for {color}")
            }
            Action::ActivateLed { color } => {
                write!(f, "Crack Lion's Eye Diamond for {color}{color}{color}, discarding hand")
            }
            Action::Choose(choice) => write!(f, "{choice}"),
        }
    }
}

fn list(cards: &[Card]) -> String {
    if cards.is_empty() {
        return "nothing".to_string();
    }
    cards
        .iter()
        .map(|c| c.name())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_support::main_phase;
    use crate::state::{Outcome, PlayerState, Puzzle};

    fn cast_action(card: Card, payment: ManaPool) -> Action {
        Action::CastSpell { card, payment }
    }

    #[test]
    fn casting_puts_spell_on_stack_and_keeps_priority() {
        let state = main_phase(PlayerState {
            hand: vec![Card::Brainstorm],
            mana_pool: ManaPool::of(Color::U, 1),
            ..PlayerState::default()
        });
        let action = cast_action(Card::Brainstorm, ManaPool::of(Color::U, 1));
        assert!(legal_actions(&state).contains(&action));
        let next = apply(&state, &action).unwrap();
        assert_eq!(
            next.stack,
            vec![StackItem::Spell {
                card: Card::Brainstorm,
                controller: Player::You,
            }]
        );
        assert!(next.you.hand.is_empty());
        assert!(next.you.mana_pool.is_empty());
        assert_eq!(next.you.spells_cast_this_turn, 1);
        assert_eq!(next.priority, Player::You);
        assert!(!next.passed_in_succession);
        // The input was not mutated.
        assert_eq!(state.you.hand, vec![Card::Brainstorm]);
    }

    #[test]
    fn two_passes_resolve_the_top_of_the_stack() {
        let mut state = main_phase(PlayerState::default());
        state.stack.push(StackItem::Spell {
            card: Card::DarkRitual,
            controller: Player::You,
        });
        let after_mine = apply(&state, &Action::Pass).unwrap();
        assert_eq!(after_mine.priority, Player::Opponent);
        assert!(after_mine.passed_in_succession);
        assert_eq!(after_mine.stack.len(), 1);
        assert_eq!(legal_actions(&after_mine), vec![Action::Pass]);
        let resolved = apply(&after_mine, &Action::Pass).unwrap();
        assert!(resolved.stack.is_empty());
        assert_eq!(resolved.you.mana_pool, ManaPool::of(Color::B, 3));
        assert_eq!(resolved.priority, Player::You);
    }

    #[test]
    fn acting_between_passes_breaks_the_run() {
        let mut state = main_phase(PlayerState {
            battlefield: vec![Permanent::untapped(Card::Island)],
            ..PlayerState::default()
        });
        state.stack.push(StackItem::Spell {
            card: Card::DarkRitual,
            controller: Player::You,
        });
        // Opponent passes first (we fake it by handing them priority).
        state.priority = Player::Opponent;
        let state = apply(&state, &Action::Pass).unwrap();
        assert!(state.passed_in_succession);
        let state = apply(
            &state,
            &Action::TapForMana {
                source: Card::Island,
                color: Color::U,
            },
        )
        .unwrap();
        assert!(!state.passed_in_succession);
        let state = apply(&state, &Action::Pass).unwrap();
        assert_eq!(state.stack.len(), 1, "one pass alone must not resolve");
    }

    #[test]
    fn both_passing_on_an_empty_stack_ends_the_turn() {
        let state = main_phase(PlayerState::default());
        let state = apply(&state, &Action::Pass).unwrap();
        let state = apply(&state, &Action::Pass).unwrap();
        assert_eq!(state.outcome, Some(Outcome::Loss(LossReason::TurnEnded)));
        assert!(legal_actions(&state).is_empty());
        assert_eq!(apply(&state, &Action::Pass), Err(EngineError::GameOver));
    }

    #[test]
    fn sorcery_speed_needs_an_empty_stack() {
        let mut state = main_phase(PlayerState {
            hand: vec![Card::Ponder, Card::Brainstorm, Card::ThassasOracle, Card::LotusPetal],
            mana_pool: ManaPool::of(Color::U, 3),
            ..PlayerState::default()
        });
        let open = legal_actions(&state);
        assert!(open.contains(&cast_action(Card::Ponder, ManaPool::of(Color::U, 1))));
        assert!(open.contains(&cast_action(Card::ThassasOracle, ManaPool::of(Color::U, 2))));
        assert!(open.contains(&cast_action(Card::LotusPetal, ManaPool::EMPTY)));

        state.stack.push(StackItem::Spell {
            card: Card::DarkRitual,
            controller: Player::You,
        });
        let busy = legal_actions(&state);
        assert!(busy.contains(&cast_action(Card::Brainstorm, ManaPool::of(Color::U, 1))));
        assert!(!busy.iter().any(|a| matches!(
            a,
            Action::CastSpell {
                card: Card::Ponder | Card::ThassasOracle | Card::LotusPetal,
                ..
            }
        )));
        assert_eq!(
            apply(&state, &cast_action(Card::Ponder, ManaPool::of(Color::U, 1))),
            Err(EngineError::IllegalAction(cast_action(
                Card::Ponder,
                ManaPool::of(Color::U, 1)
            )))
        );
    }

    #[test]
    fn cannot_cast_without_the_mana() {
        let state = main_phase(PlayerState {
            hand: vec![Card::ThassasOracle],
            mana_pool: ManaPool::of(Color::U, 1).with(Color::B, 3),
            ..PlayerState::default()
        });
        assert!(!legal_actions(&state).iter().any(|a| matches!(
            a,
            Action::CastSpell {
                card: Card::ThassasOracle,
                ..
            }
        )));
    }

    #[test]
    fn land_drop_only_once_and_only_at_sorcery_speed() {
        let state = main_phase(PlayerState {
            hand: vec![Card::Island, Card::Swamp],
            ..PlayerState::default()
        });
        assert!(legal_actions(&state).contains(&Action::PlayLand(Card::Island)));
        let played = apply(&state, &Action::PlayLand(Card::Island)).unwrap();
        assert_eq!(played.you.battlefield, vec![Permanent::untapped(Card::Island)]);
        assert!(!played.you.land_drop_available);
        assert!(!legal_actions(&played).contains(&Action::PlayLand(Card::Swamp)));

        let mut busy = state.clone();
        busy.stack.push(StackItem::Spell {
            card: Card::DarkRitual,
            controller: Player::You,
        });
        assert!(!legal_actions(&busy).contains(&Action::PlayLand(Card::Island)));
    }

    #[test]
    fn lands_and_moxen_tap_for_their_colors() {
        let state = main_phase(PlayerState {
            battlefield: vec![
                Permanent::untapped(Card::UndergroundSea),
                Permanent::untapped(Card::MoxJet),
                Permanent::tapped(Card::Island),
            ],
            ..PlayerState::default()
        });
        let actions = legal_actions(&state);
        let taps: Vec<&Action> = actions
            .iter()
            .filter(|a| matches!(a, Action::TapForMana { .. }))
            .collect();
        assert_eq!(
            taps,
            vec![
                &Action::TapForMana {
                    source: Card::MoxJet,
                    color: Color::B,
                },
                &Action::TapForMana {
                    source: Card::UndergroundSea,
                    color: Color::U,
                },
                &Action::TapForMana {
                    source: Card::UndergroundSea,
                    color: Color::B,
                },
            ]
        );
        let next = apply(
            &state,
            &Action::TapForMana {
                source: Card::UndergroundSea,
                color: Color::B,
            },
        )
        .unwrap();
        assert_eq!(next.you.mana_pool, ManaPool::of(Color::B, 1));
        assert!(next.you.battlefield.iter().all(|p| p.tapped || p.card == Card::MoxJet));
        // A tapped land can't be tapped again.
        assert!(!legal_actions(&next).iter().any(|a| matches!(
            a,
            Action::TapForMana {
                source: Card::UndergroundSea,
                ..
            }
        )));
    }

    #[test]
    fn identical_permanents_give_one_action_each() {
        let state = main_phase(PlayerState {
            battlefield: vec![
                Permanent::untapped(Card::Island),
                Permanent::untapped(Card::Island),
            ],
            ..PlayerState::default()
        });
        let taps = legal_actions(&state)
            .iter()
            .filter(|a| matches!(a, Action::TapForMana { .. }))
            .count();
        assert_eq!(taps, 1);
    }

    #[test]
    fn lotus_and_petal_sacrifice_for_any_color() {
        let state = main_phase(PlayerState {
            battlefield: vec![
                Permanent::untapped(Card::BlackLotus),
                Permanent::untapped(Card::LotusPetal),
            ],
            ..PlayerState::default()
        });
        let lotus = apply(
            &state,
            &Action::SacrificeForMana {
                source: Card::BlackLotus,
                color: Color::U,
            },
        )
        .unwrap();
        assert_eq!(lotus.you.mana_pool, ManaPool::of(Color::U, 3));
        assert_eq!(lotus.you.graveyard, vec![Card::BlackLotus]);
        assert_eq!(lotus.you.battlefield, vec![Permanent::untapped(Card::LotusPetal)]);
        let petal = apply(
            &lotus,
            &Action::SacrificeForMana {
                source: Card::LotusPetal,
                color: Color::G,
            },
        )
        .unwrap();
        assert_eq!(petal.you.mana_pool, ManaPool::of(Color::U, 3).with(Color::G, 1));
        assert!(petal.you.battlefield.is_empty());
    }

    #[test]
    fn led_discards_hand_and_works_with_a_spell_on_the_stack() {
        let mut state = main_phase(PlayerState {
            hand: vec![Card::Island, Card::Swamp],
            battlefield: vec![Permanent::untapped(Card::LionsEyeDiamond)],
            ..PlayerState::default()
        });
        state.stack.push(StackItem::Spell {
            card: Card::Brainstorm,
            controller: Player::You,
        });
        let action = Action::ActivateLed { color: Color::U };
        assert!(legal_actions(&state).contains(&action));
        let next = apply(&state, &action).unwrap();
        assert!(next.you.hand.is_empty());
        assert_eq!(next.you.mana_pool, ManaPool::of(Color::U, 3));
        assert_eq!(
            next.you.graveyard,
            vec![Card::LionsEyeDiamond, Card::Island, Card::Swamp]
        );
        assert!(next.you.battlefield.is_empty());
        assert_eq!(next.stack.len(), 1, "mana abilities don't use the stack");
        assert_eq!(next.priority, Player::You);
    }

    #[test]
    fn led_with_an_empty_hand_is_legal() {
        let state = main_phase(PlayerState {
            battlefield: vec![Permanent::untapped(Card::LionsEyeDiamond)],
            ..PlayerState::default()
        });
        let next = apply(&state, &Action::ActivateLed { color: Color::B }).unwrap();
        assert_eq!(next.you.mana_pool, ManaPool::of(Color::B, 3));
    }

    #[test]
    fn led_is_not_available_while_a_choice_is_pending() {
        let state = main_phase(PlayerState {
            hand: vec![Card::Island, Card::Swamp],
            battlefield: vec![Permanent::untapped(Card::LionsEyeDiamond)],
            ..PlayerState::default()
        });
        let mut state = state;
        state.pending = Some(crate::state::PendingChoice {
            player: Player::You,
            card: Card::Brainstorm,
            kind: crate::state::ChoiceKind::BrainstormPutBack,
        });
        let actions = legal_actions(&state);
        assert!(!actions.is_empty());
        assert!(actions.iter().all(|a| matches!(a, Action::Choose(_))));
        assert!(matches!(
            apply(&state, &Action::ActivateLed { color: Color::U }),
            Err(EngineError::IllegalAction(_))
        ));
    }

    #[test]
    fn mana_persists_across_resolutions() {
        let state = main_phase(PlayerState {
            hand: vec![Card::DarkRitual],
            mana_pool: ManaPool::of(Color::B, 1).with(Color::U, 2),
            ..PlayerState::default()
        });
        let state = apply(&state, &cast_action(Card::DarkRitual, ManaPool::of(Color::B, 1))).unwrap();
        let state = apply(&apply(&state, &Action::Pass).unwrap(), &Action::Pass).unwrap();
        assert_eq!(state.you.mana_pool, ManaPool::of(Color::B, 3).with(Color::U, 2));
    }

    fn puzzle_with_candidates(candidates: Vec<Card>, graveyard: Vec<Card>, life: i16) -> Puzzle {
        Puzzle {
            you: PlayerState {
                life,
                library: candidates,
                graveyard,
                ..PlayerState::default()
            },
            opponent: PlayerState::default(),
        }
    }

    #[test]
    fn doomsday_pile_from_library_and_graveyard() {
        let puzzle = puzzle_with_candidates(
            vec![Card::Island, Card::Swamp, Card::Plains, Card::ThassasOracle],
            vec![Card::Brainstorm, Card::Ponder],
            20,
        );
        let pile = [
            Card::Brainstorm,
            Card::ThassasOracle,
            Card::Island,
            Card::Swamp,
            Card::Plains,
        ];
        let state = State::with_pile(puzzle, &pile).unwrap();
        assert_eq!(state.phase, Phase::Main);
        assert_eq!(state.you.library, pile.to_vec());
        assert!(state.you.library_ordered);
        assert_eq!(state.you.graveyard, vec![Card::Doomsday]);
        assert_eq!(state.you.exile, vec![Card::Ponder]);
        assert_eq!(state.you.life, 10);
        assert_eq!(state.priority, Player::You);
        assert!(state.stack.is_empty());
    }

    #[test]
    fn doomsday_life_loss_rounds_up() {
        let puzzle = puzzle_with_candidates(vec![Card::ThassasOracle], vec![], 7);
        let state = State::with_pile(puzzle, &[Card::ThassasOracle]).unwrap();
        assert_eq!(state.you.life, 3);
    }

    #[test]
    fn doomsday_with_fewer_than_five_takes_them_all() {
        let puzzle = puzzle_with_candidates(vec![Card::ThassasOracle, Card::Island], vec![], 20);
        let choices = legal_actions(&State::new(puzzle.clone()));
        assert_eq!(choices.len(), 2);
        assert!(matches!(
            State::with_pile(puzzle, &[Card::ThassasOracle]),
            Err(EngineError::IllegalAction(_))
        ));
    }

    #[test]
    fn doomsday_must_pick_exactly_five_that_exist() {
        let puzzle = puzzle_with_candidates(
            vec![Card::Island, Card::Island, Card::Swamp, Card::Plains, Card::Forest, Card::Mountain],
            vec![],
            20,
        );
        // Two Islands are available, three are not.
        assert!(matches!(
            State::with_pile(
                puzzle.clone(),
                &[Card::Island, Card::Island, Card::Island, Card::Swamp, Card::Plains]
            ),
            Err(EngineError::IllegalAction(_))
        ));
        assert!(State::with_pile(
            puzzle.clone(),
            &[Card::Island, Card::Island, Card::Swamp, Card::Plains, Card::Forest]
        )
        .is_ok());
        // 6 candidates with a duplicate: 6!/1! orderings minus the repeats.
        let choices = legal_actions(&State::new(puzzle));
        assert_eq!(choices.len(), 720 - 360);
    }

    #[test]
    fn ordered_selections_dedupe_repeats() {
        let sel = ordered_selections(&[Card::Island, Card::Island, Card::Swamp], 2);
        assert_eq!(
            sel,
            vec![
                vec![Card::Island, Card::Island],
                vec![Card::Island, Card::Swamp],
                vec![Card::Swamp, Card::Island],
            ]
        );
        assert_eq!(ordered_selections(&[], 0), vec![Vec::<Card>::new()]);
    }
}
