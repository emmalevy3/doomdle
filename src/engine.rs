//! Actions, legality and the state transition function.
//!
//! [`legal_actions`] is the only place that decides what a player may do.
//! [`apply`] is pure: it returns a new state and never mutates its input.

use std::fmt;

use crate::cards::{self, AbilityKind, Card, CardKind, CyclingCost, LoyaltyAbility, ManaAbility};
use crate::mana::{AltCost, Color, ManaPool, Payment};
use crate::state::{
    LossReason, Permanent, Phase, Player, PlayerState, StackItem, State, Target, Zone,
};

/// A player's answer to a [`crate::state::PendingChoice`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Choice {
    /// Two cards from hand to put on top of the library: `[top, second]`.
    BrainstormPutBack([Card; 2]),
    /// The new order of the looked-at cards, top first.
    PonderOrder(Vec<Card>),
    /// Cards kept on top (in order) and cards sent to the bottom (in order).
    PreordainScry { top: Vec<Card>, bottom: Vec<Card> },
    /// Surveil 1 (Consider, Undercity Sewers, Meticulous Archive).
    Surveil { to_graveyard: bool },
    /// The land found by a search, or `None` to find nothing.
    SearchLand(Option<Card>),
    /// Flow State: the looked-at cards going to hand, and the rest in the
    /// order they go to the bottom (first element ends up highest).
    FlowState {
        to_hand: Vec<Card>,
        bottom: Vec<Card>,
    },
    /// Frantic Search: the cards discarded from hand.
    Discard(Vec<Card>),
    /// Frantic Search: the tapped lands to untap.
    UntapLands(Vec<Card>),
    /// Pay this mana so Daze, Spell Pierce or Miscalculation doesn't counter.
    PayTax(ManaPool),
    /// Don't pay: the spell is countered.
    DeclineTax,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    /// Doomsday's pile, top first. Only legal in [`Phase::ChoosingPile`].
    ChoosePile(Vec<Card>),
    Pass,
    /// Cast `card` from `from`, paying exactly `payment`. `target` is set for
    /// targeted spells (Unearth, the counters) and `None` otherwise.
    CastSpell {
        card: Card,
        payment: Payment,
        target: Option<Target>,
        from: Zone,
    },
    PlayLand {
        card: Card,
        from: Zone,
    },
    /// Cycle `card` from hand. `sacrifice` names the land for Edge of Autumn.
    Cycle {
        card: Card,
        payment: Payment,
        sacrifice: Option<Card>,
    },
    /// Crack an untapped fetchland: {T}, pay 1 life, sacrifice it.
    ActivateFetch {
        source: Card,
    },
    /// Doctor Doom's {T} ability.
    ActivateDoom,
    /// A planeswalker's loyalty ability (sorcery speed, once per turn).
    Loyalty {
        source: Card,
        ability: LoyaltyAbility,
    },
    /// Tap an untapped permanent with this card name for one mana.
    TapForMana {
        source: Card,
        color: Color,
    },
    /// Black Lotus or Lotus Petal.
    SacrificeForMana {
        source: Card,
        color: Color,
    },
    /// Lion's Eye Diamond: discard your hand, add three of `color`.
    ActivateLed {
        color: Color,
    },
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
        Player::Opponent => their_actions(state),
    }
}

/// Ordered so that passing, casting and activating come before mana
/// abilities: the solver tries actions in this order, and resolving or
/// casting what you already can pay for beats floating mana you may never use.
fn your_actions(state: &State) -> Vec<Action> {
    let you = &state.you;
    let sorcery_speed = state.stack.is_empty();
    let mut out = vec![Action::Pass];

    for card in distinct(&you.hand) {
        push_plays(
            &mut out,
            state,
            Player::You,
            card,
            Zone::Hand,
            sorcery_speed,
        );
        push_cycling(&mut out, you, card);
    }
    // Yawgmoth's Will: the graveyard is a second hand for casting and playing
    // (not for cycling, which is a discard).
    if you.yawgmoths_will {
        for card in distinct(&you.graveyard) {
            push_plays(
                &mut out,
                state,
                Player::You,
                card,
                Zone::Graveyard,
                sorcery_speed,
            );
        }
    }

    for perm in distinct(&you.battlefield) {
        if perm.card.fetch_filter().is_some() && !perm.tapped && you.life >= 1 {
            out.push(Action::ActivateFetch { source: perm.card });
        }
        if perm.card == Card::DoctorDoom && !perm.tapped && !perm.summoning_sick {
            out.push(Action::ActivateDoom);
        }
        if sorcery_speed && !perm.loyalty_used {
            for ability in perm.card.loyalty_abilities() {
                out.push(Action::Loyalty {
                    source: perm.card,
                    ability,
                });
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

/// The opponent only responds: a targeted instant from hand aimed at something
/// of yours on the stack, paid by tapping their lands as part of the cast, or
/// a pass. Casts come first so the AND search meets the strongest branch
/// early.
fn their_actions(state: &State) -> Vec<Action> {
    let mut out = Vec::new();
    for card in distinct(&state.opponent.hand) {
        if card.kind() == CardKind::Instant
            && card.target_options(state, Player::Opponent).is_some()
        {
            push_casts(&mut out, state, Player::Opponent, card, Zone::Hand);
        }
    }
    out.push(Action::Pass);
    out
}

/// Casting or playing `card` from `from`, respecting timing.
fn push_plays(
    out: &mut Vec<Action>,
    state: &State,
    player: Player,
    card: Card,
    from: Zone,
    sorcery_speed: bool,
) {
    match card.kind() {
        CardKind::Land => {
            if sorcery_speed && state.player(player).land_drop_available {
                out.push(Action::PlayLand { card, from });
            }
        }
        CardKind::Instant => push_casts(out, state, player, card, from),
        CardKind::Sorcery | CardKind::Creature | CardKind::Artifact | CardKind::Planeswalker => {
            if sorcery_speed {
                push_casts(out, state, player, card, from);
            }
        }
    }
}

/// One cast per way to pay, times one per legal target for targeted spells.
fn push_casts(out: &mut Vec<Action>, state: &State, player: Player, card: Card, from: Zone) {
    if !card.castable() {
        return;
    }
    let targets: Vec<Option<Target>> = match card.target_options(state, player) {
        None => vec![None],
        Some(targets) => targets.into_iter().map(Some).collect(),
    };
    for payment in cast_payments(state, player, card) {
        for &target in &targets {
            out.push(Action::CastSpell {
                card,
                payment,
                target,
                from,
            });
        }
    }
}

/// Every way `player` could pay for `card` right now: its mana cost from the
/// pool (for the opponent, also from lands tapped on the spot), plus its
/// alternative costs.
fn cast_payments(state: &State, player: Player, card: Card) -> Vec<Payment> {
    let ps = state.player(player);
    let pools = match player {
        Player::You => vec![ps.mana_pool],
        Player::Opponent => reachable_pools(ps),
    };
    let mut out = Vec::new();
    for pool in &pools {
        for payment in Payment::options(pool, &card.mana_cost(), ps.life) {
            if !out.contains(&payment) {
                out.push(payment);
            }
        }
    }
    out.extend(card.alternative_payments(state, player));
    out
}

/// Takes `mana` out of the pool, first tapping lands for any shortfall (the
/// opponent pays that way; your own payments are only ever offered from mana
/// already in the pool). `None` if it can't be paid.
pub(crate) fn pay_mana(ps: &mut PlayerState, mana: &ManaPool) -> Option<()> {
    if ps.mana_pool.minus(mana).is_none() {
        for (idx, color) in tap_plan(ps, mana)? {
            ps.battlefield[idx].tapped = true;
            ps.mana_pool.add_mana(color, 1);
        }
    }
    ps.mana_pool = ps.mana_pool.minus(mana)?;
    Some(())
}

/// The ways `player` can pay a generic `amount` right now: you from your pool,
/// the opponent also by tapping lands.
pub(crate) fn tax_payments(state: &State, player: Player, amount: u8) -> Vec<ManaPool> {
    let ps = state.player(player);
    let cost = crate::mana::ManaCost::FREE.with_generic(amount);
    let pools = match player {
        Player::You => vec![ps.mana_pool],
        Player::Opponent => reachable_pools(ps),
    };
    let mut out = Vec::new();
    for pool in &pools {
        for mana in pool.payments_for(&cost) {
            if !out.contains(&mana) {
                out.push(mana);
            }
        }
    }
    out
}

/// The untapped permanents of `ps` that tap for mana, as (index, colors).
fn tap_sources(ps: &PlayerState) -> Vec<(usize, Vec<Color>)> {
    ps.battlefield
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.tapped && p.card.mana_ability() == Some(ManaAbility::Tap))
        .map(|(i, p)| (i, p.card.mana_colors()))
        .collect()
}

/// Every pool `ps` can reach by tapping any of its mana sources for any of
/// their colors, the current pool included. Used for the opponent, who taps
/// lands only as part of casting.
fn reachable_pools(ps: &PlayerState) -> Vec<ManaPool> {
    let mut out = Vec::new();
    for_each_tap_plan(&tap_sources(ps), &mut |taps| {
        let pool = pool_after(ps, taps);
        if !out.contains(&pool) {
            out.push(pool);
        }
    });
    out
}

/// The fewest taps that give `ps` a pool covering `needed`, in the same
/// enumeration order as [`reachable_pools`] so the two always agree.
fn tap_plan(ps: &PlayerState, needed: &ManaPool) -> Option<Vec<(usize, Color)>> {
    let mut best: Option<Vec<(usize, Color)>> = None;
    for_each_tap_plan(&tap_sources(ps), &mut |taps| {
        let covers = pool_after(ps, taps).minus(needed).is_some();
        if covers && best.as_ref().is_none_or(|b| taps.len() < b.len()) {
            best = Some(taps.to_vec());
        }
    });
    best
}

fn pool_after(ps: &PlayerState, taps: &[(usize, Color)]) -> ManaPool {
    let mut pool = ps.mana_pool;
    for &(_, color) in taps {
        pool.add_mana(color, 1);
    }
    pool
}

/// Calls `visit` with every way to tap a subset of `sources`, each for one of
/// its colors (the empty plan included).
fn for_each_tap_plan(sources: &[(usize, Vec<Color>)], visit: &mut impl FnMut(&[(usize, Color)])) {
    fn go(
        sources: &[(usize, Vec<Color>)],
        acc: &mut Vec<(usize, Color)>,
        visit: &mut impl FnMut(&[(usize, Color)]),
    ) {
        let Some(((idx, colors), rest)) = sources.split_first() else {
            visit(acc);
            return;
        };
        go(rest, acc, visit);
        for &color in colors {
            acc.push((*idx, color));
            go(rest, acc, visit);
            acc.pop();
        }
    }
    go(sources, &mut Vec::new(), visit);
}

fn push_cycling(out: &mut Vec<Action>, you: &PlayerState, card: Card) {
    match card.cycling_cost() {
        None => {}
        Some(CyclingCost::Mana(cost)) => {
            for mana in you.mana_pool.payments_for(&cost) {
                out.push(Action::Cycle {
                    card,
                    payment: Payment::mana(mana),
                    sacrifice: None,
                });
            }
        }
        Some(CyclingCost::Life(n)) => {
            if you.life >= i16::from(n) {
                out.push(Action::Cycle {
                    card,
                    payment: Payment::life(n),
                    sacrifice: None,
                });
            }
        }
        Some(CyclingCost::SacrificeLand) => {
            let lands: Vec<Card> = you
                .battlefield
                .iter()
                .filter(|p| p.card.kind() == CardKind::Land)
                .map(|p| p.card)
                .collect();
            for land in distinct(&lands) {
                out.push(Action::Cycle {
                    card,
                    payment: Payment::FREE,
                    sacrifice: Some(land),
                });
            }
        }
    }
}

/// Sorted, deduplicated copy: identical cards or permanents give one action.
fn distinct<T: Ord + Clone>(items: &[T]) -> Vec<T> {
    let mut v = items.to_vec();
    v.sort();
    v.dedup();
    v
}

/// Every distinct unordered selection of `k` items from a multiset, each
/// returned sorted.
pub(crate) fn unordered_selections(items: &[Card], k: usize) -> Vec<Vec<Card>> {
    let mut out: Vec<Vec<Card>> = ordered_selections(items, k)
        .into_iter()
        .map(|mut sel| {
            sel.sort();
            sel
        })
        .collect();
    out.sort();
    out.dedup();
    out
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
            Action::CastSpell {
                card,
                payment,
                target,
                from,
            } => cast(&mut next, actor, *card, payment, *target, *from)?,
            Action::PlayLand { card, from } => play_land(&mut next, actor, *card, *from)?,
            Action::Cycle {
                card,
                payment,
                sacrifice,
            } => cycle(&mut next, actor, *card, payment, *sacrifice)?,
            Action::ActivateFetch { source } => activate_fetch(&mut next, actor, *source)?,
            Action::ActivateDoom => activate_doom(&mut next, actor)?,
            Action::Loyalty { source, ability } => {
                activate_loyalty(&mut next, actor, *source, *ability)?
            }
            Action::TapForMana { source, color } => {
                tap_for_mana(&mut next, actor, *source, *color)?
            }
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
    // Doomsday goes to the graveyard after resolving (or to exile, if
    // Yawgmoth's Will was already active).
    you.graveyard.clear();
    you.put_in_graveyard(Card::Doomsday);
    you.life -= (you.life.max(0) + 1) / 2;
    // Doomsday itself was cast this turn; the puzzle only lists the spells
    // cast before it.
    you.spells_cast_this_turn += 1;

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
        StackItem::Spell {
            card,
            controller,
            target,
        } => match card.kind() {
            CardKind::Creature | CardKind::Artifact | CardKind::Planeswalker => {
                state.enter_battlefield(controller, card, false);
                Ok(())
            }
            CardKind::Instant | CardKind::Sorcery => {
                cards::resolve_spell(state, card, controller, target)
            }
            CardKind::Land => Err(EngineError::Internal("a land can't be on the stack")),
        },
        StackItem::Trigger {
            kind, controller, ..
        } => cards::resolve_trigger(state, kind, controller),
        StackItem::Ability {
            source,
            kind,
            controller,
        } => cards::resolve_ability(state, kind, source, controller),
    }
}

fn cast(
    state: &mut State,
    player: Player,
    card: Card,
    payment: &Payment,
    target: Option<Target>,
    from: Zone,
) -> Result<(), EngineError> {
    let illegal = || {
        EngineError::IllegalAction(Action::CastSpell {
            card,
            payment: *payment,
            target,
            from,
        })
    };
    if !card.castable() || !cast_payments(state, player, card).contains(payment) {
        return Err(illegal());
    }
    let target_ok = match (card.target_options(state, player), target) {
        (None, None) => true,
        (Some(options), Some(target)) => options.contains(&target),
        _ => false,
    };
    if !target_ok {
        return Err(illegal());
    }
    let ps = state.player_mut(player);
    pay_mana(ps, &payment.mana).ok_or_else(illegal)?;
    ps.life -= i16::from(payment.life);
    take_from_zone(ps, card, from).ok_or_else(illegal)?;
    match payment.alt {
        Some(AltCost::Pitch(pitched)) => {
            ps.remove_from_hand(pitched).ok_or_else(illegal)?;
            ps.exile.push(pitched);
        }
        Some(AltCost::ReturnIsland(island)) => {
            let idx = ps
                .battlefield
                .iter()
                .position(|p| p.card == island)
                .ok_or_else(illegal)?;
            ps.battlefield.remove(idx);
            ps.hand.push(island);
        }
        None => {}
    }
    ps.spells_cast_this_turn += 1;
    state.stack.push(StackItem::Spell {
        card,
        controller: player,
        target,
    });
    acted(state, player);
    Ok(())
}

/// Removes the stack item at `index` and keeps every other item's stack
/// target pointing at the same item: indices above shift down, and a target
/// on the removed item itself is cleared so that spell fizzles.
pub(crate) fn remove_stack_item(state: &mut State, index: usize) -> StackItem {
    let removed = state.stack.remove(index);
    for item in &mut state.stack {
        if let StackItem::Spell {
            target: Some(Target::Stack { index: i, .. }),
            ..
        } = item
        {
            if *i == index {
                if let StackItem::Spell { target, .. } = item {
                    *target = None;
                }
            } else if *i > index {
                *i -= 1;
            }
        }
    }
    removed
}

/// Removes one copy of `card` from `from`, if it is there and that zone may
/// be cast or played from right now.
fn take_from_zone(ps: &mut PlayerState, card: Card, from: Zone) -> Option<Card> {
    match from {
        Zone::Hand => ps.remove_from_hand(card),
        Zone::Graveyard if ps.yawgmoths_will => ps.remove_from_graveyard(card),
        Zone::Graveyard => None,
    }
}

fn play_land(state: &mut State, player: Player, card: Card, from: Zone) -> Result<(), EngineError> {
    let illegal = || EngineError::IllegalAction(Action::PlayLand { card, from });
    let ps = state.player_mut(player);
    if !ps.land_drop_available || card.kind() != CardKind::Land {
        return Err(illegal());
    }
    take_from_zone(ps, card, from).ok_or_else(illegal)?;
    ps.land_drop_available = false;
    state.enter_battlefield(player, card, card.enters_tapped());
    acted(state, player);
    Ok(())
}

/// Cycling: pay the cost, discard the card, put "draw a card" on the stack.
/// Not a spell, so it neither counts for storm nor can be countered.
fn cycle(
    state: &mut State,
    player: Player,
    card: Card,
    payment: &Payment,
    sacrifice: Option<Card>,
) -> Result<(), EngineError> {
    let illegal = || {
        EngineError::IllegalAction(Action::Cycle {
            card,
            payment: *payment,
            sacrifice,
        })
    };
    let cost = card.cycling_cost().ok_or_else(illegal)?;
    match (cost, sacrifice) {
        (CyclingCost::Mana(cost), None) => {
            let ps = state.player_mut(player);
            if payment.life != 0 || !ps.mana_pool.payments_for(&cost).contains(&payment.mana) {
                return Err(illegal());
            }
            ps.mana_pool = ps.mana_pool.minus(&payment.mana).ok_or_else(illegal)?;
        }
        (CyclingCost::Life(n), None) => {
            let ps = state.player_mut(player);
            if *payment != Payment::life(n) || ps.life < i16::from(n) {
                return Err(illegal());
            }
            ps.life -= i16::from(n);
        }
        (CyclingCost::SacrificeLand, Some(land)) => {
            if !payment.is_free() || land.kind() != CardKind::Land {
                return Err(illegal());
            }
            let idx = find_permanent(state, player, land, |_| true).ok_or_else(illegal)?;
            let ps = state.player_mut(player);
            ps.battlefield.remove(idx);
            ps.put_in_graveyard(land);
        }
        _ => return Err(illegal()),
    }
    let ps = state.player_mut(player);
    ps.remove_from_hand(card).ok_or_else(illegal)?;
    ps.put_in_graveyard(card);
    state.stack.push(StackItem::Ability {
        source: card,
        kind: AbilityKind::Cycling,
        controller: player,
    });
    acted(state, player);
    Ok(())
}

fn activate_fetch(state: &mut State, player: Player, source: Card) -> Result<(), EngineError> {
    let illegal = || EngineError::IllegalAction(Action::ActivateFetch { source });
    if source.fetch_filter().is_none() || state.player(player).life < 1 {
        return Err(illegal());
    }
    let idx = find_permanent(state, player, source, |p| !p.tapped).ok_or_else(illegal)?;
    let ps = state.player_mut(player);
    ps.life -= 1;
    ps.battlefield.remove(idx);
    ps.put_in_graveyard(source);
    state.stack.push(StackItem::Ability {
        source,
        kind: AbilityKind::Fetch,
        controller: player,
    });
    acted(state, player);
    Ok(())
}

fn activate_doom(state: &mut State, player: Player) -> Result<(), EngineError> {
    let source = Card::DoctorDoom;
    let idx = find_permanent(state, player, source, |p| !p.tapped && !p.summoning_sick)
        .ok_or(EngineError::IllegalAction(Action::ActivateDoom))?;
    state.player_mut(player).battlefield[idx].tapped = true;
    state.stack.push(StackItem::Ability {
        source,
        kind: AbilityKind::DoomDraw,
        controller: player,
    });
    acted(state, player);
    Ok(())
}

fn activate_loyalty(
    state: &mut State,
    player: Player,
    source: Card,
    ability: LoyaltyAbility,
) -> Result<(), EngineError> {
    let illegal = || EngineError::IllegalAction(Action::Loyalty { source, ability });
    if !source.loyalty_abilities().contains(&ability) || !state.stack.is_empty() {
        return Err(illegal());
    }
    let idx = find_permanent(state, player, source, |p| !p.loyalty_used).ok_or_else(illegal)?;
    let perm = &mut state.player_mut(player).battlefield[idx];
    let loyalty = i16::from(perm.loyalty) + i16::from(ability.loyalty_change());
    perm.loyalty = u8::try_from(loyalty).map_err(|_| illegal())?;
    perm.loyalty_used = true;
    state.stack.push(StackItem::Ability {
        source,
        kind: ability.ability(),
        controller: player,
    });
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
    ps.put_in_graveyard(source);
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
    for card in discarded {
        ps.put_in_graveyard(card);
    }
    ps.put_in_graveyard(source);
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
            Choice::Surveil { to_graveyard } => {
                if *to_graveyard {
                    write!(f, "Surveil: top card to graveyard")
                } else {
                    write!(f, "Surveil: keep top card")
                }
            }
            Choice::SearchLand(Some(card)) => write!(f, "Find {card}"),
            Choice::SearchLand(None) => write!(f, "Find nothing"),
            Choice::FlowState { to_hand, bottom } => {
                write!(
                    f,
                    "Flow State: take {} into hand, {} to the bottom",
                    list(to_hand),
                    list(bottom)
                )
            }
            Choice::Discard(cards) => write!(f, "Discard {}", list(cards)),
            Choice::UntapLands(cards) => write!(f, "Untap {}", list(cards)),
            Choice::PayTax(mana) => write!(f, "Pay {mana} to keep the spell"),
            Choice::DeclineTax => write!(f, "Don't pay: let the spell be countered"),
        }
    }
}

impl fmt::Display for Zone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Zone::Hand => f.write_str("hand"),
            Zone::Graveyard => f.write_str("graveyard"),
        }
    }
}

impl fmt::Display for LoyaltyAbility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoyaltyAbility::JacePlusOne {
                target: Player::You,
            } => f.write_str("+1 targeting yourself"),
            LoyaltyAbility::JacePlusOne {
                target: Player::Opponent,
            } => f.write_str("+1 targeting the opponent"),
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::ChoosePile(pile) => write!(f, "Doomsday pile (top first): {}", list(pile)),
            Action::Pass => write!(f, "Pass priority"),
            Action::CastSpell {
                card,
                payment,
                target,
                from,
            } => {
                write!(f, "Cast {card}")?;
                if *from != Zone::Hand {
                    write!(f, " from {from}")?;
                }
                if let Some(target) = target {
                    write!(f, " targeting {target}")?;
                }
                if payment.is_free() {
                    write!(f, " for free")
                } else {
                    write!(f, " paying {payment}")
                }
            }
            Action::PlayLand { card, from } => {
                write!(f, "Play {card}")?;
                if *from != Zone::Hand {
                    write!(f, " from {from}")?;
                }
                Ok(())
            }
            Action::Cycle {
                card,
                payment,
                sacrifice,
            } => {
                write!(f, "Cycle {card}")?;
                if let Some(land) = sacrifice {
                    write!(f, " sacrificing {land}")?;
                }
                if !payment.is_free() {
                    write!(f, " paying {payment}")?;
                }
                Ok(())
            }
            Action::ActivateFetch { source } => write!(f, "Crack {source}"),
            Action::ActivateDoom => write!(f, "Tap Doctor Doom to draw"),
            Action::Loyalty { source, ability } => write!(f, "{source}: {ability}"),
            Action::TapForMana { source, color } => write!(f, "Tap {source} for {color}"),
            Action::SacrificeForMana { source, color } => {
                write!(f, "Sacrifice {source} for {color}")
            }
            Action::ActivateLed { color } => {
                write!(
                    f,
                    "Crack Lion's Eye Diamond for {color}{color}{color}, discarding hand"
                )
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
    use crate::cards::TriggerKind;
    use crate::mana::ManaPool;
    use crate::state::test_support::{main_phase, versus};
    use crate::state::{ChoiceKind, Outcome, PendingChoice, PlayerState, Puzzle};

    fn cast_action(card: Card, payment: ManaPool) -> Action {
        Action::CastSpell {
            card,
            payment: payment.into(),
            target: None,
            from: Zone::Hand,
        }
    }

    fn play_land_action(card: Card) -> Action {
        Action::PlayLand {
            card,
            from: Zone::Hand,
        }
    }

    fn pass_twice(state: &State) -> State {
        apply(&apply(state, &Action::Pass).unwrap(), &Action::Pass).unwrap()
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
                target: None,
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
            target: None,
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
            target: None,
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
            hand: vec![
                Card::Ponder,
                Card::Brainstorm,
                Card::ThassasOracle,
                Card::LotusPetal,
                Card::JaceWielderOfMysteries,
            ],
            mana_pool: ManaPool::of(Color::U, 4),
            ..PlayerState::default()
        });
        let open = legal_actions(&state);
        assert!(open.contains(&cast_action(Card::Ponder, ManaPool::of(Color::U, 1))));
        assert!(open.contains(&cast_action(Card::ThassasOracle, ManaPool::of(Color::U, 2))));
        assert!(open.contains(&cast_action(Card::LotusPetal, ManaPool::EMPTY)));
        assert!(open.contains(&cast_action(
            Card::JaceWielderOfMysteries,
            ManaPool::of(Color::U, 4)
        )));

        state.stack.push(StackItem::Spell {
            card: Card::DarkRitual,
            controller: Player::You,
            target: None,
        });
        let busy = legal_actions(&state);
        assert!(busy.contains(&cast_action(Card::Brainstorm, ManaPool::of(Color::U, 1))));
        assert!(!busy.iter().any(|a| matches!(
            a,
            Action::CastSpell {
                card: Card::Ponder
                    | Card::ThassasOracle
                    | Card::LotusPetal
                    | Card::JaceWielderOfMysteries,
                ..
            }
        )));
        assert_eq!(
            apply(
                &state,
                &cast_action(Card::Ponder, ManaPool::of(Color::U, 1))
            ),
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
    fn probe_is_cast_for_blue_or_two_life() {
        let state = main_phase(PlayerState {
            hand: vec![Card::GitaxianProbe],
            mana_pool: ManaPool::of(Color::U, 1),
            library: vec![Card::Island],
            life: 10,
            ..PlayerState::default()
        });
        let casts = legal_actions(&state)
            .iter()
            .filter(|a| matches!(a, Action::CastSpell { .. }))
            .count();
        assert_eq!(casts, 2);
        let for_life = Action::CastSpell {
            card: Card::GitaxianProbe,
            payment: Payment::life(2),
            target: None,
            from: Zone::Hand,
        };
        let next = apply(&state, &for_life).unwrap();
        assert_eq!(next.you.life, 8);
        assert_eq!(next.you.mana_pool, ManaPool::of(Color::U, 1));
        assert_eq!(next.you.spells_cast_this_turn, 1, "still a spell for storm");
        assert_eq!(pass_twice(&next).you.hand, vec![Card::Island]);
    }

    #[test]
    fn life_cannot_be_paid_below_what_you_have() {
        let one_life = main_phase(PlayerState {
            hand: vec![Card::GitaxianProbe, Card::StreetWraith],
            life: 1,
            ..PlayerState::default()
        });
        assert_eq!(legal_actions(&one_life), vec![Action::Pass]);
    }

    #[test]
    fn paying_life_down_to_zero_loses_after_the_action() {
        let state = main_phase(PlayerState {
            hand: vec![Card::GitaxianProbe],
            library: vec![Card::Island],
            life: 2,
            ..PlayerState::default()
        });
        let next = apply(
            &state,
            &Action::CastSpell {
                card: Card::GitaxianProbe,
                payment: Payment::life(2),
                target: None,
                from: Zone::Hand,
            },
        )
        .unwrap();
        assert_eq!(next.outcome, Some(Outcome::Loss(LossReason::LifeZero)));
    }

    #[test]
    fn street_wraith_cycles_for_two_life_and_is_never_cast() {
        let state = main_phase(PlayerState {
            hand: vec![Card::StreetWraith],
            mana_pool: ManaPool::of(Color::B, 5),
            library: vec![Card::Island],
            spells_cast_this_turn: 1,
            life: 5,
            ..PlayerState::default()
        });
        let actions = legal_actions(&state);
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, Action::CastSpell { .. }))
        );
        let cycle = Action::Cycle {
            card: Card::StreetWraith,
            payment: Payment::life(2),
            sacrifice: None,
        };
        assert!(actions.contains(&cycle));
        let next = apply(&state, &cycle).unwrap();
        assert_eq!(next.you.life, 3);
        assert!(next.you.hand.is_empty());
        assert_eq!(next.you.graveyard, vec![Card::StreetWraith]);
        assert_eq!(next.you.spells_cast_this_turn, 1, "cycling is not a spell");
        assert_eq!(
            next.stack,
            vec![StackItem::Ability {
                source: Card::StreetWraith,
                kind: AbilityKind::Cycling,
                controller: Player::You,
            }]
        );
        assert_eq!(pass_twice(&next).you.hand, vec![Card::Island]);
    }

    #[test]
    fn cycling_is_instant_speed() {
        let mut state = main_phase(PlayerState {
            hand: vec![Card::StreetWraith],
            library: vec![Card::Island],
            ..PlayerState::default()
        });
        state.stack.push(StackItem::Spell {
            card: Card::DarkRitual,
            controller: Player::You,
            target: None,
        });
        assert!(
            legal_actions(&state)
                .iter()
                .any(|a| matches!(a, Action::Cycle { .. }))
        );
    }

    #[test]
    fn edge_of_autumn_cycles_by_sacrificing_any_land() {
        let state = main_phase(PlayerState {
            hand: vec![Card::EdgeOfAutumn],
            battlefield: vec![
                Permanent::tapped(Card::Island),
                Permanent::untapped(Card::Swamp),
                Permanent::untapped(Card::LotusPetal),
            ],
            library: vec![Card::Plains],
            ..PlayerState::default()
        });
        let cycles: Vec<Action> = legal_actions(&state)
            .into_iter()
            .filter(|a| matches!(a, Action::Cycle { .. }))
            .collect();
        assert_eq!(
            cycles,
            vec![
                Action::Cycle {
                    card: Card::EdgeOfAutumn,
                    payment: Payment::FREE,
                    sacrifice: Some(Card::Island),
                },
                Action::Cycle {
                    card: Card::EdgeOfAutumn,
                    payment: Payment::FREE,
                    sacrifice: Some(Card::Swamp),
                },
            ]
        );
        let next = apply(&state, &cycles[0]).unwrap();
        assert_eq!(
            next.you.battlefield,
            vec![
                Permanent::untapped(Card::LotusPetal),
                Permanent::untapped(Card::Swamp),
            ]
        );
        assert_eq!(next.you.graveyard, vec![Card::EdgeOfAutumn, Card::Island]);
        assert_eq!(pass_twice(&next).you.hand, vec![Card::Plains]);
    }

    #[test]
    fn edge_of_autumn_cannot_cycle_without_a_land() {
        let state = main_phase(PlayerState {
            hand: vec![Card::EdgeOfAutumn],
            battlefield: vec![Permanent::untapped(Card::LotusPetal)],
            ..PlayerState::default()
        });
        assert!(
            !legal_actions(&state)
                .iter()
                .any(|a| matches!(a, Action::Cycle { .. }))
        );
    }

    #[test]
    fn fetchland_costs_a_tap_a_life_and_itself() {
        let state = main_phase(PlayerState {
            battlefield: vec![
                Permanent::untapped(Card::PollutedDelta),
                Permanent::tapped(Card::FloodedStrand),
            ],
            library: vec![Card::Island, Card::Swamp],
            life: 3,
            ..PlayerState::default()
        });
        let actions = legal_actions(&state);
        assert!(actions.contains(&Action::ActivateFetch {
            source: Card::PollutedDelta
        }));
        assert!(!actions.contains(&Action::ActivateFetch {
            source: Card::FloodedStrand
        }));
        let next = apply(
            &state,
            &Action::ActivateFetch {
                source: Card::PollutedDelta,
            },
        )
        .unwrap();
        assert_eq!(next.you.life, 2);
        assert_eq!(
            next.you.battlefield,
            vec![Permanent::tapped(Card::FloodedStrand)]
        );
        assert_eq!(next.you.graveyard, vec![Card::PollutedDelta]);
        assert_eq!(
            next.stack,
            vec![StackItem::Ability {
                source: Card::PollutedDelta,
                kind: AbilityKind::Fetch,
                controller: Player::You,
            }]
        );
        let resolved = pass_twice(&next);
        assert_eq!(
            resolved.pending.unwrap().kind,
            ChoiceKind::SearchLand(crate::cards::LandFilter::Types(
                Card::PollutedDelta.fetch_filter().unwrap()
            ))
        );
        let found = apply(
            &resolved,
            &Action::Choose(Choice::SearchLand(Some(Card::Swamp))),
        )
        .unwrap();
        assert!(
            found
                .you
                .battlefield
                .contains(&Permanent::untapped(Card::Swamp))
        );
        assert_eq!(found.you.library, vec![Card::Island]);
        assert_eq!(found.pending, None);
    }

    #[test]
    fn fetching_at_one_life_is_legal_but_fatal() {
        let state = main_phase(PlayerState {
            battlefield: vec![Permanent::untapped(Card::PollutedDelta)],
            life: 1,
            ..PlayerState::default()
        });
        let action = Action::ActivateFetch {
            source: Card::PollutedDelta,
        };
        assert!(legal_actions(&state).contains(&action));
        let next = apply(&state, &action).unwrap();
        assert_eq!(next.outcome, Some(Outcome::Loss(LossReason::LifeZero)));

        let mut dead = state;
        dead.you.life = 0;
        assert!(!legal_actions(&dead).contains(&action));
    }

    #[test]
    fn surveil_land_enters_tapped_and_triggers() {
        let state = main_phase(PlayerState {
            hand: vec![Card::UndercitySewers],
            library: vec![Card::Island, Card::Swamp],
            ..PlayerState::default()
        });
        let played = apply(&state, &play_land_action(Card::UndercitySewers)).unwrap();
        assert_eq!(
            played.you.battlefield,
            vec![Permanent::tapped(Card::UndercitySewers)]
        );
        assert!(!played.you.land_drop_available);
        assert_eq!(
            played.stack,
            vec![StackItem::Trigger {
                source: Card::UndercitySewers,
                kind: TriggerKind::Surveil,
                controller: Player::You,
            }]
        );
        // No mana from a tapped land.
        assert!(
            !legal_actions(&played)
                .iter()
                .any(|a| matches!(a, Action::TapForMana { .. }))
        );
        let choice = pass_twice(&played);
        let binned = apply(
            &choice,
            &Action::Choose(Choice::Surveil { to_graveyard: true }),
        )
        .unwrap();
        assert_eq!(binned.you.library, vec![Card::Swamp]);
        assert_eq!(binned.you.graveyard, vec![Card::Island]);
    }

    #[test]
    fn doom_activation_needs_an_untapped_unsick_doom() {
        let ready = main_phase(PlayerState {
            battlefield: vec![Permanent::untapped(Card::DoctorDoom)],
            library: vec![Card::Island],
            ..PlayerState::default()
        });
        assert!(legal_actions(&ready).contains(&Action::ActivateDoom));
        let next = apply(&ready, &Action::ActivateDoom).unwrap();
        assert_eq!(
            next.you.battlefield,
            vec![Permanent::tapped(Card::DoctorDoom)]
        );
        assert_eq!(
            next.stack,
            vec![StackItem::Ability {
                source: Card::DoctorDoom,
                kind: AbilityKind::DoomDraw,
                controller: Player::You,
            }]
        );
        assert!(!legal_actions(&next).contains(&Action::ActivateDoom));

        let tapped = main_phase(PlayerState {
            battlefield: vec![Permanent::tapped(Card::DoctorDoom)],
            ..PlayerState::default()
        });
        assert!(!legal_actions(&tapped).contains(&Action::ActivateDoom));

        let sick = main_phase(PlayerState {
            battlefield: vec![Permanent::entering(Card::DoctorDoom, false)],
            ..PlayerState::default()
        });
        assert!(!legal_actions(&sick).contains(&Action::ActivateDoom));
    }

    #[test]
    fn doom_cast_from_hand_is_summoning_sick() {
        let state = main_phase(PlayerState {
            hand: vec![Card::DoctorDoom],
            mana_pool: ManaPool::of(Color::B, 4),
            ..PlayerState::default()
        });
        let cast = apply(
            &state,
            &cast_action(Card::DoctorDoom, ManaPool::of(Color::B, 4)),
        )
        .unwrap();
        let resolved = pass_twice(&cast);
        assert!(resolved.you.battlefield[0].summoning_sick);
        assert!(!legal_actions(&resolved).contains(&Action::ActivateDoom));
    }

    #[test]
    fn jace_loyalty_is_sorcery_speed_and_once_per_turn() {
        let state = main_phase(PlayerState {
            battlefield: vec![Permanent::untapped(Card::JaceWielderOfMysteries)],
            library: vec![Card::Island, Card::Swamp, Card::Plains],
            ..PlayerState::default()
        });
        let plus_you = Action::Loyalty {
            source: Card::JaceWielderOfMysteries,
            ability: LoyaltyAbility::JacePlusOne {
                target: Player::You,
            },
        };
        let plus_them = Action::Loyalty {
            source: Card::JaceWielderOfMysteries,
            ability: LoyaltyAbility::JacePlusOne {
                target: Player::Opponent,
            },
        };
        let actions = legal_actions(&state);
        assert!(actions.contains(&plus_you));
        assert!(actions.contains(&plus_them));

        let next = apply(&state, &plus_you).unwrap();
        let jace = next.you.battlefield[0];
        assert_eq!(jace.loyalty, 5);
        assert!(jace.loyalty_used);
        assert_eq!(
            next.stack,
            vec![StackItem::Ability {
                source: Card::JaceWielderOfMysteries,
                kind: AbilityKind::JacePlusOne {
                    target: Player::You
                },
                controller: Player::You,
            }]
        );
        // Not again this turn, and not with something on the stack.
        assert!(
            !legal_actions(&next)
                .iter()
                .any(|a| matches!(a, Action::Loyalty { .. }))
        );
        let resolved = pass_twice(&next);
        assert!(
            !legal_actions(&resolved)
                .iter()
                .any(|a| matches!(a, Action::Loyalty { .. }))
        );

        let mut busy = state.clone();
        busy.stack.push(StackItem::Spell {
            card: Card::DarkRitual,
            controller: Player::You,
            target: None,
        });
        assert!(
            !legal_actions(&busy)
                .iter()
                .any(|a| matches!(a, Action::Loyalty { .. }))
        );
    }

    #[test]
    fn jace_can_activate_the_turn_he_enters() {
        let state = main_phase(PlayerState {
            hand: vec![Card::JaceWielderOfMysteries],
            mana_pool: ManaPool::of(Color::U, 4),
            library: vec![Card::Island, Card::Swamp],
            ..PlayerState::default()
        });
        let cast = apply(
            &state,
            &cast_action(Card::JaceWielderOfMysteries, ManaPool::of(Color::U, 4)),
        )
        .unwrap();
        let resolved = pass_twice(&cast);
        assert_eq!(resolved.you.battlefield[0].loyalty, 4);
        assert!(
            legal_actions(&resolved)
                .iter()
                .any(|a| matches!(a, Action::Loyalty { .. }))
        );
    }

    #[test]
    fn land_drop_only_once_and_only_at_sorcery_speed() {
        let state = main_phase(PlayerState {
            hand: vec![Card::Island, Card::Swamp],
            ..PlayerState::default()
        });
        assert!(legal_actions(&state).contains(&play_land_action(Card::Island)));
        let played = apply(&state, &play_land_action(Card::Island)).unwrap();
        assert_eq!(
            played.you.battlefield,
            vec![Permanent::untapped(Card::Island)]
        );
        assert!(!played.you.land_drop_available);
        assert!(!legal_actions(&played).contains(&play_land_action(Card::Swamp)));

        let mut busy = state.clone();
        busy.stack.push(StackItem::Spell {
            card: Card::DarkRitual,
            controller: Player::You,
            target: None,
        });
        assert!(!legal_actions(&busy).contains(&play_land_action(Card::Island)));
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
        assert!(
            next.you
                .battlefield
                .iter()
                .all(|p| p.tapped || p.card == Card::MoxJet)
        );
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
        assert_eq!(
            lotus.you.battlefield,
            vec![Permanent::untapped(Card::LotusPetal)]
        );
        let petal = apply(
            &lotus,
            &Action::SacrificeForMana {
                source: Card::LotusPetal,
                color: Color::G,
            },
        )
        .unwrap();
        assert_eq!(
            petal.you.mana_pool,
            ManaPool::of(Color::U, 3).with(Color::G, 1)
        );
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
            target: None,
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
        let mut state = main_phase(PlayerState {
            hand: vec![Card::Island, Card::Swamp],
            battlefield: vec![Permanent::untapped(Card::LionsEyeDiamond)],
            ..PlayerState::default()
        });
        state.pending = Some(PendingChoice {
            player: Player::You,
            kind: ChoiceKind::BrainstormPutBack,
            bin: Some(Card::Brainstorm),
            bin_owner: Player::You,
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
        let state = apply(
            &state,
            &cast_action(Card::DarkRitual, ManaPool::of(Color::B, 1)),
        )
        .unwrap();
        let state = pass_twice(&state);
        assert_eq!(
            state.you.mana_pool,
            ManaPool::of(Color::B, 3).with(Color::U, 2)
        );
    }

    #[test]
    fn actions_are_ordered_pass_casts_abilities_then_mana() {
        let state = main_phase(PlayerState {
            hand: vec![Card::StreetWraith, Card::Brainstorm],
            battlefield: vec![
                Permanent::untapped(Card::Island),
                Permanent::untapped(Card::PollutedDelta),
            ],
            mana_pool: ManaPool::of(Color::U, 1),
            ..PlayerState::default()
        });
        let actions = legal_actions(&state);
        let pos = |pred: fn(&Action) -> bool| actions.iter().position(pred).unwrap();
        let pass = pos(|a| matches!(a, Action::Pass));
        let cast = pos(|a| matches!(a, Action::CastSpell { .. }));
        let cycle = pos(|a| matches!(a, Action::Cycle { .. }));
        let fetch = pos(|a| matches!(a, Action::ActivateFetch { .. }));
        let tap = pos(|a| matches!(a, Action::TapForMana { .. }));
        assert!(pass < cast && cast < fetch && cycle < fetch && fetch < tap);
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
        assert_eq!(
            state.you.spells_cast_this_turn, 1,
            "Doomsday counts for storm"
        );
        assert_eq!(state.priority, Player::You);
        assert!(state.stack.is_empty());
    }

    #[test]
    fn doomsday_adds_to_spells_cast_before_it() {
        let mut puzzle = puzzle_with_candidates(vec![Card::ThassasOracle], vec![], 20);
        puzzle.you.spells_cast_this_turn = 2;
        let state = State::with_pile(puzzle, &[Card::ThassasOracle]).unwrap();
        assert_eq!(state.you.spells_cast_this_turn, 3);
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
            vec![
                Card::Island,
                Card::Island,
                Card::Swamp,
                Card::Plains,
                Card::Forest,
                Card::Mountain,
            ],
            vec![],
            20,
        );
        // Two Islands are available, three are not.
        assert!(matches!(
            State::with_pile(
                puzzle.clone(),
                &[
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Swamp,
                    Card::Plains
                ]
            ),
            Err(EngineError::IllegalAction(_))
        ));
        assert!(
            State::with_pile(
                puzzle.clone(),
                &[
                    Card::Island,
                    Card::Island,
                    Card::Swamp,
                    Card::Plains,
                    Card::Forest
                ]
            )
            .is_ok()
        );
        // 6 candidates with a duplicate: 6!/1! orderings minus the repeats.
        let choices = legal_actions(&State::new(puzzle));
        assert_eq!(choices.len(), 720 - 360);
    }

    #[test]
    fn unearth_needs_a_target_and_offers_one_cast_per_target() {
        let no_target = main_phase(PlayerState {
            hand: vec![Card::Unearth],
            mana_pool: ManaPool::of(Color::B, 1),
            graveyard: vec![Card::Doomsday],
            ..PlayerState::default()
        });
        assert!(
            !legal_actions(&no_target)
                .iter()
                .any(|a| matches!(a, Action::CastSpell { .. }))
        );

        let mut state = no_target;
        state.you.graveyard = vec![
            Card::Doomsday,
            Card::ThassasOracle,
            Card::LaboratoryManiac,
            Card::DoctorDoom,
        ];
        let cast = |target| Action::CastSpell {
            card: Card::Unearth,
            payment: ManaPool::of(Color::B, 1).into(),
            target: Some(Target::Card(target)),
            from: Zone::Hand,
        };
        let casts: Vec<Action> = legal_actions(&state)
            .into_iter()
            .filter(|a| matches!(a, Action::CastSpell { .. }))
            .collect();
        assert_eq!(
            casts,
            vec![cast(Card::ThassasOracle), cast(Card::LaboratoryManiac)]
        );
        let next = apply(&state, &casts[0]).unwrap();
        assert_eq!(
            next.stack,
            vec![StackItem::Spell {
                card: Card::Unearth,
                controller: Player::You,
                target: Some(Target::Card(Card::ThassasOracle)),
            }]
        );
        assert!(matches!(
            apply(&state, &cast(Card::DoctorDoom)),
            Err(EngineError::IllegalAction(_))
        ));
    }

    #[test]
    fn unearth_cycles_for_two_generic_at_instant_speed() {
        let mut state = main_phase(PlayerState {
            hand: vec![Card::Unearth],
            mana_pool: ManaPool::of(Color::U, 1).with(Color::G, 1),
            library: vec![Card::Island],
            ..PlayerState::default()
        });
        state.stack.push(StackItem::Spell {
            card: Card::DarkRitual,
            controller: Player::You,
            target: None,
        });
        let cycle = Action::Cycle {
            card: Card::Unearth,
            payment: Payment::mana(ManaPool::of(Color::U, 1).with(Color::G, 1)),
            sacrifice: None,
        };
        assert!(legal_actions(&state).contains(&cycle));
        let next = apply(&state, &cycle).unwrap();
        assert!(next.you.mana_pool.is_empty());
        assert_eq!(next.you.graveyard, vec![Card::Unearth]);
        assert_eq!(next.stack.len(), 2);

        let poor = main_phase(PlayerState {
            hand: vec![Card::Unearth],
            mana_pool: ManaPool::of(Color::B, 1),
            library: vec![Card::Island],
            ..PlayerState::default()
        });
        assert!(
            !legal_actions(&poor)
                .iter()
                .any(|a| matches!(a, Action::Cycle { .. }))
        );
    }

    #[test]
    fn yawgmoths_will_lets_you_cast_and_play_from_the_graveyard() {
        let state = main_phase(PlayerState {
            yawgmoths_will: true,
            graveyard: vec![
                Card::Brainstorm,
                Card::Ponder,
                Card::Island,
                Card::StreetWraith,
                Card::Doomsday,
            ],
            mana_pool: ManaPool::of(Color::U, 2).with(Color::B, 3),
            ..PlayerState::default()
        });
        let from_graveyard = |card| Action::CastSpell {
            card,
            payment: ManaPool::of(Color::U, 1).into(),
            target: None,
            from: Zone::Graveyard,
        };
        let island = Action::PlayLand {
            card: Card::Island,
            from: Zone::Graveyard,
        };
        let actions = legal_actions(&state);
        assert!(actions.contains(&from_graveyard(Card::Brainstorm)));
        assert!(actions.contains(&from_graveyard(Card::Ponder)));
        assert!(actions.contains(&island));
        // No cycling from the graveyard, no second Doomsday, and nothing
        // "from hand" when the hand is empty.
        assert!(!actions.iter().any(|a| matches!(a, Action::Cycle { .. })));
        assert!(!actions.iter().any(|a| matches!(
            a,
            Action::CastSpell {
                card: Card::Doomsday,
                ..
            }
        )));
        assert!(!actions.iter().any(|a| matches!(
            a,
            Action::CastSpell {
                from: Zone::Hand,
                ..
            } | Action::PlayLand {
                from: Zone::Hand,
                ..
            }
        )));

        let cast = apply(&state, &from_graveyard(Card::Ponder)).unwrap();
        assert_eq!(
            cast.you.graveyard,
            vec![
                Card::Doomsday,
                Card::Brainstorm,
                Card::StreetWraith,
                Card::Island
            ]
        );
        assert_eq!(cast.you.spells_cast_this_turn, 1);
        // Timing still applies: an instant yes, a land drop no.
        let busy = legal_actions(&cast);
        assert!(busy.contains(&from_graveyard(Card::Brainstorm)));
        assert!(!busy.contains(&island));

        let played = apply(&state, &island).unwrap();
        assert_eq!(
            played.you.battlefield,
            vec![Permanent::untapped(Card::Island)]
        );
        assert!(!played.you.land_drop_available);
    }

    #[test]
    fn the_graveyard_zone_is_illegal_without_yawgmoths_will() {
        let state = main_phase(PlayerState {
            graveyard: vec![Card::Brainstorm],
            mana_pool: ManaPool::of(Color::U, 1),
            ..PlayerState::default()
        });
        let action = Action::CastSpell {
            card: Card::Brainstorm,
            payment: ManaPool::of(Color::U, 1).into(),
            target: None,
            from: Zone::Graveyard,
        };
        assert!(!legal_actions(&state).contains(&action));
        assert_eq!(
            apply(&state, &action),
            Err(EngineError::IllegalAction(action.clone()))
        );
        // And a card that is only in the graveyard can't be cast "from hand".
        let mut will = state;
        will.you.yawgmoths_will = true;
        assert!(
            !legal_actions(&will)
                .contains(&cast_action(Card::Brainstorm, ManaPool::of(Color::U, 1)))
        );
    }

    #[test]
    fn classic_will_line_recasts_led_and_the_discarded_hand() {
        let state = main_phase(PlayerState {
            hand: vec![Card::YawgmothsWill, Card::ThassasOracle],
            battlefield: vec![Permanent::untapped(Card::LionsEyeDiamond)],
            mana_pool: ManaPool::of(Color::B, 3),
            library: vec![Card::Island],
            ..PlayerState::default()
        });
        let state = apply(
            &state,
            &cast_action(Card::YawgmothsWill, ManaPool::of(Color::B, 3)),
        )
        .unwrap();
        // Hold priority and crack LED: Oracle and LED hit the graveyard first.
        let state = apply(&state, &Action::ActivateLed { color: Color::U }).unwrap();
        assert_eq!(
            state.you.graveyard,
            vec![Card::ThassasOracle, Card::LionsEyeDiamond]
        );
        let state = pass_twice(&state);
        assert!(state.you.yawgmoths_will);
        assert_eq!(state.you.exile, vec![Card::YawgmothsWill]);

        let recast_led = Action::CastSpell {
            card: Card::LionsEyeDiamond,
            payment: Payment::FREE,
            target: None,
            from: Zone::Graveyard,
        };
        let state = pass_twice(&apply(&state, &recast_led).unwrap());
        assert_eq!(
            state.you.battlefield,
            vec![Permanent::entering(Card::LionsEyeDiamond, false)]
        );
        let state = apply(&state, &Action::ActivateLed { color: Color::U }).unwrap();
        assert_eq!(state.you.mana_pool, ManaPool::of(Color::U, 6));
        assert_eq!(
            state.you.exile,
            vec![Card::YawgmothsWill, Card::LionsEyeDiamond],
            "the second LED is exiled, not binned"
        );

        let oracle = Action::CastSpell {
            card: Card::ThassasOracle,
            payment: ManaPool::of(Color::U, 2).into(),
            target: None,
            from: Zone::Graveyard,
        };
        let state = pass_twice(&pass_twice(&apply(&state, &oracle).unwrap()));
        assert_eq!(state.outcome, Some(Outcome::Win));
    }

    #[test]
    fn will_before_doomsday_exiles_doomsday_itself() {
        let mut puzzle = puzzle_with_candidates(vec![Card::ThassasOracle], vec![], 20);
        puzzle.you.yawgmoths_will = true;
        let state = State::with_pile(puzzle, &[Card::ThassasOracle]).unwrap();
        assert!(state.you.graveyard.is_empty());
        assert_eq!(state.you.exile, vec![Card::Doomsday]);
    }

    #[test]
    fn unordered_selections_dedupe_and_sort() {
        assert_eq!(
            unordered_selections(&[Card::Island, Card::Island, Card::Swamp], 2),
            vec![
                vec![Card::Island, Card::Island],
                vec![Card::Island, Card::Swamp],
            ]
        );
        assert_eq!(
            unordered_selections(&[Card::Swamp, Card::Island], 1),
            vec![vec![Card::Island], vec![Card::Swamp]]
        );
        assert_eq!(unordered_selections(&[], 0), vec![Vec::<Card>::new()]);
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

    // ------------------------------------------------------------ stage 4a

    fn opponent_with(hand: Vec<Card>, battlefield: Vec<Permanent>) -> PlayerState {
        PlayerState {
            hand,
            battlefield,
            ..PlayerState::default()
        }
    }

    fn your_spell(card: Card) -> StackItem {
        StackItem::Spell {
            card,
            controller: Player::You,
            target: None,
        }
    }

    fn their_spell(card: Card, target: Option<Target>) -> StackItem {
        StackItem::Spell {
            card,
            controller: Player::Opponent,
            target,
        }
    }

    fn on_stack(index: usize, card: Card) -> Target {
        Target::Stack { index, card }
    }

    /// You have cast `spell` and passed priority to the opponent.
    fn facing(you: PlayerState, opponent: PlayerState, spell: Card) -> State {
        let mut state = versus(you, opponent);
        state.stack.push(your_spell(spell));
        apply(&state, &Action::Pass).unwrap()
    }

    fn their_cast(card: Card, payment: Payment, target: Target) -> Action {
        Action::CastSpell {
            card,
            payment,
            target: Some(target),
            from: Zone::Hand,
        }
    }

    #[test]
    fn opponent_with_nothing_to_cast_can_only_pass() {
        let state = facing(
            PlayerState::default(),
            PlayerState::default(),
            Card::Brainstorm,
        );
        assert_eq!(legal_actions(&state), vec![Action::Pass]);
        // A counter with nothing of yours on the stack is uncastable.
        let mut idle = versus(
            PlayerState::default(),
            opponent_with(
                vec![Card::Counterspell],
                vec![Permanent::untapped(Card::Island); 2],
            ),
        );
        idle.priority = Player::Opponent;
        assert_eq!(legal_actions(&idle), vec![Action::Pass]);
    }

    #[test]
    fn opponent_counterspell_taps_two_islands_as_part_of_casting() {
        let islands = vec![Permanent::untapped(Card::Island); 2];
        let state = facing(
            PlayerState::default(),
            opponent_with(vec![Card::Counterspell], islands),
            Card::Brainstorm,
        );
        let counter = their_cast(
            Card::Counterspell,
            Payment::mana(ManaPool::of(Color::U, 2)),
            on_stack(0, Card::Brainstorm),
        );
        assert_eq!(legal_actions(&state), vec![counter.clone(), Action::Pass]);
        let next = apply(&state, &counter).unwrap();
        assert_eq!(
            next.opponent.battlefield,
            vec![Permanent::tapped(Card::Island); 2]
        );
        assert!(next.opponent.mana_pool.is_empty());
        assert!(next.opponent.hand.is_empty());
        assert_eq!(next.opponent.spells_cast_this_turn, 1);
        assert_eq!(
            next.stack,
            vec![
                your_spell(Card::Brainstorm),
                their_spell(Card::Counterspell, Some(on_stack(0, Card::Brainstorm))),
            ]
        );
        // The caster keeps priority.
        assert_eq!(next.priority, Player::Opponent);
        assert!(!next.passed_in_succession);
    }

    #[test]
    fn opponent_cannot_cast_what_their_lands_cannot_pay() {
        let state = facing(
            PlayerState::default(),
            opponent_with(
                vec![Card::Counterspell],
                vec![Permanent::untapped(Card::Island)],
            ),
            Card::Brainstorm,
        );
        assert_eq!(legal_actions(&state), vec![Action::Pass]);
        // A tapped land is no help either.
        let state = facing(
            PlayerState::default(),
            opponent_with(
                vec![Card::Counterspell],
                vec![
                    Permanent::untapped(Card::Island),
                    Permanent::tapped(Card::Island),
                ],
            ),
            Card::Brainstorm,
        );
        assert_eq!(legal_actions(&state), vec![Action::Pass]);
    }

    #[test]
    fn opponent_only_ever_casts_instants_in_response() {
        let state = facing(
            PlayerState::default(),
            opponent_with(
                vec![
                    Card::Island,
                    Card::Miscalculation,
                    Card::Unearth,
                    Card::Brainstorm,
                    Card::ThassasOracle,
                ],
                vec![
                    Permanent::untapped(Card::Island),
                    Permanent::untapped(Card::MoxSapphire),
                ],
            ),
            Card::Brainstorm,
        );
        // Miscalculation from Island + Mox; no land play, tap, cycle, or
        // Brainstorm (nothing to target) and no sorcery-speed spells.
        assert_eq!(
            legal_actions(&state),
            vec![
                their_cast(
                    Card::Miscalculation,
                    Payment::mana(ManaPool::of(Color::U, 2)),
                    on_stack(0, Card::Brainstorm),
                ),
                Action::Pass,
            ]
        );
    }

    #[test]
    fn reachable_pools_and_tap_plan_agree() {
        let ps = PlayerState {
            battlefield: vec![
                Permanent::untapped(Card::Island),
                Permanent::untapped(Card::Island),
                Permanent::untapped(Card::Tundra),
                Permanent::tapped(Card::Swamp),
            ],
            ..PlayerState::default()
        };
        let pools = reachable_pools(&ps);
        let u = |n| ManaPool::of(Color::U, n);
        for pool in [
            ManaPool::EMPTY,
            u(1),
            u(2),
            u(3),
            ManaPool::of(Color::W, 1),
            u(2).with(Color::W, 1),
        ] {
            assert!(pools.contains(&pool), "{pool} should be reachable");
        }
        assert!(!pools.contains(&ManaPool::of(Color::B, 1)));
        assert!(!pools.contains(&u(4)));
        // Fewest taps: UU needs two lands, W just the Tundra.
        assert_eq!(tap_plan(&ps, &u(2)).unwrap().len(), 2);
        assert_eq!(
            tap_plan(&ps, &ManaPool::of(Color::W, 1)),
            Some(vec![(2, Color::W)])
        );
        assert_eq!(tap_plan(&ps, &u(4)), None);
        // Mana already floating counts.
        let floating = PlayerState {
            mana_pool: u(1),
            ..ps
        };
        assert_eq!(tap_plan(&floating, &u(2)).unwrap().len(), 1);
    }

    #[test]
    fn nobody_targets_their_own_stack_items() {
        let mut state = versus(
            PlayerState {
                hand: vec![Card::PactOfNegation],
                ..PlayerState::default()
            },
            opponent_with(vec![Card::Stifle], vec![Permanent::untapped(Card::Island)]),
        );
        state.stack.push(your_spell(Card::Brainstorm));
        state.stack.push(their_spell(
            Card::Counterspell,
            Some(on_stack(0, Card::Brainstorm)),
        ));
        state.stack.push(StackItem::Trigger {
            source: Card::ThassasOracle,
            kind: TriggerKind::OracleEnters,
            controller: Player::You,
        });
        assert_eq!(
            Card::PactOfNegation.target_options(&state, Player::You),
            Some(vec![on_stack(1, Card::Counterspell)])
        );
        assert_eq!(
            Card::Stifle.target_options(&state, Player::Opponent),
            Some(vec![on_stack(2, Card::ThassasOracle)])
        );
        assert_eq!(
            Card::Stifle.target_options(&state, Player::You),
            Some(vec![])
        );
        // Your Pact is a real action for free.
        assert!(legal_actions(&state).contains(&Action::CastSpell {
            card: Card::PactOfNegation,
            payment: Payment::FREE,
            target: Some(on_stack(1, Card::Counterspell)),
            from: Zone::Hand,
        }));
    }

    #[test]
    fn counter_filters_follow_the_oracle_text() {
        let mut state = main_phase(PlayerState::default());
        for card in [
            Card::ThassasOracle,
            Card::Brainstorm,
            Card::FlowState,
            Card::LionsEyeDiamond,
            Card::GitaxianProbe,
        ] {
            state.stack.push(your_spell(card));
        }
        state.stack.push(StackItem::Trigger {
            source: Card::ThassasOracle,
            kind: TriggerKind::OracleEnters,
            controller: Player::You,
        });
        state.stack.push(StackItem::Ability {
            source: Card::Unearth,
            kind: AbilityKind::Cycling,
            controller: Player::You,
        });
        let targets = |card: Card| -> Vec<Card> {
            card.target_options(&state, Player::Opponent)
                .unwrap()
                .into_iter()
                .map(|t| match t {
                    Target::Stack { card, .. } => card,
                    Target::Card(card) => card,
                })
                .collect()
        };
        let all_spells = vec![
            Card::ThassasOracle,
            Card::Brainstorm,
            Card::FlowState,
            Card::LionsEyeDiamond,
            Card::GitaxianProbe,
        ];
        for any in [
            Card::Counterspell,
            Card::ForceOfWill,
            Card::Daze,
            Card::Miscalculation,
            Card::PactOfNegation,
        ] {
            assert_eq!(targets(any), all_spells, "{any}");
        }
        for noncreature in [Card::SpellPierce, Card::ForceOfNegation] {
            assert_eq!(targets(noncreature), all_spells[1..], "{noncreature}");
        }
        // Probe is {U/P}: mana value 1 however it is paid.
        assert_eq!(
            targets(Card::MentalMisstep),
            vec![Card::Brainstorm, Card::GitaxianProbe]
        );
        // Oracle is {U}{U}: Spell Snare's one creature target in this pool.
        assert_eq!(
            targets(Card::SpellSnare),
            vec![Card::ThassasOracle, Card::FlowState]
        );
        assert_eq!(
            targets(Card::Stifle),
            vec![Card::ThassasOracle, Card::Unearth]
        );
    }

    #[test]
    fn force_of_will_pitches_a_blue_card_for_one_life() {
        let hand = vec![
            Card::ForceOfWill,
            Card::ForceOfWill,
            Card::Brainstorm,
            Card::Swamp,
        ];
        let state = facing(
            PlayerState::default(),
            opponent_with(hand, Vec::new()),
            Card::ThassasOracle,
        );
        let pitch = |card| {
            their_cast(
                Card::ForceOfWill,
                Payment {
                    alt: Some(AltCost::Pitch(card)),
                    ..Payment::life(1)
                },
                on_stack(0, Card::ThassasOracle),
            )
        };
        // Another Force or Brainstorm, never the Swamp, never itself twice.
        assert_eq!(
            legal_actions(&state),
            vec![
                pitch(Card::ForceOfWill),
                pitch(Card::Brainstorm),
                Action::Pass
            ]
        );
        let next = apply(&state, &pitch(Card::Brainstorm)).unwrap();
        assert_eq!(next.opponent.life, 19);
        assert_eq!(next.opponent.exile, vec![Card::Brainstorm]);
        assert_eq!(next.opponent.hand, vec![Card::ForceOfWill, Card::Swamp]);
        assert_eq!(
            next.stack[1],
            their_spell(Card::ForceOfWill, Some(on_stack(0, Card::ThassasOracle)))
        );
        // No blue card left: no Force.
        let state = facing(
            PlayerState::default(),
            opponent_with(vec![Card::ForceOfWill, Card::Swamp], Vec::new()),
            Card::ThassasOracle,
        );
        assert_eq!(legal_actions(&state), vec![Action::Pass]);
    }

    #[test]
    fn force_of_will_at_one_life_kills_the_opponent() {
        let state = facing(
            PlayerState::default(),
            PlayerState {
                life: 1,
                ..opponent_with(vec![Card::ForceOfWill, Card::Brainstorm], Vec::new())
            },
            Card::ThassasOracle,
        );
        let force = their_cast(
            Card::ForceOfWill,
            Payment {
                alt: Some(AltCost::Pitch(Card::Brainstorm)),
                ..Payment::life(1)
            },
            on_stack(0, Card::ThassasOracle),
        );
        assert!(legal_actions(&state).contains(&force));
        let next = apply(&state, &force).unwrap();
        assert_eq!(next.opponent.life, 0);
        assert_eq!(next.outcome, Some(Outcome::Win));
    }

    #[test]
    fn force_of_negation_is_free_only_for_the_opponent() {
        let state = facing(
            PlayerState::default(),
            opponent_with(vec![Card::ForceOfNegation, Card::Ponder], Vec::new()),
            Card::Brainstorm,
        );
        let pitch = their_cast(
            Card::ForceOfNegation,
            Payment {
                alt: Some(AltCost::Pitch(Card::Ponder)),
                ..Payment::FREE
            },
            on_stack(0, Card::Brainstorm),
        );
        assert_eq!(legal_actions(&state), vec![pitch, Action::Pass]);
        // On your turn you pay 1UU for it, and it can't hit a creature.
        let mut mine = versus(
            PlayerState {
                hand: vec![Card::ForceOfNegation, Card::Ponder],
                mana_pool: ManaPool::of(Color::U, 3),
                ..PlayerState::default()
            },
            PlayerState::default(),
        );
        mine.stack.push(their_spell(Card::Counterspell, None));
        let casts: Vec<Action> = legal_actions(&mine)
            .into_iter()
            .filter(|a| matches!(a, Action::CastSpell { .. }))
            .collect();
        assert_eq!(
            casts,
            vec![Action::CastSpell {
                card: Card::ForceOfNegation,
                payment: Payment::mana(ManaPool::of(Color::U, 3)),
                target: Some(on_stack(0, Card::Counterspell)),
                from: Zone::Hand,
            }]
        );
    }

    #[test]
    fn daze_returns_an_island_to_hand_instead_of_paying() {
        let state = facing(
            PlayerState::default(),
            opponent_with(
                vec![Card::Daze],
                vec![
                    Permanent::tapped(Card::Island),
                    Permanent::untapped(Card::UndergroundSea),
                    Permanent::untapped(Card::Swamp),
                ],
            ),
            Card::Brainstorm,
        );
        let bounce = |land| {
            their_cast(
                Card::Daze,
                Payment {
                    alt: Some(AltCost::ReturnIsland(land)),
                    ..Payment::FREE
                },
                on_stack(0, Card::Brainstorm),
            )
        };
        // Hard cast from Sea + Swamp, or bounce either Island-typed land
        // (tapped or not); the Swamp is not an Island.
        assert_eq!(
            legal_actions(&state),
            vec![
                their_cast(
                    Card::Daze,
                    Payment::mana(ManaPool::of(Color::U, 1).with(Color::B, 1)),
                    on_stack(0, Card::Brainstorm),
                ),
                bounce(Card::Island),
                bounce(Card::UndergroundSea),
                Action::Pass,
            ]
        );
        let next = apply(&state, &bounce(Card::UndergroundSea)).unwrap();
        assert_eq!(next.opponent.hand, vec![Card::UndergroundSea]);
        assert_eq!(
            next.opponent.battlefield,
            vec![
                Permanent::tapped(Card::Island),
                Permanent::untapped(Card::Swamp),
            ]
        );
        assert!(next.opponent.mana_pool.is_empty());
    }

    #[test]
    fn your_daze_bounces_your_island_for_a_replay() {
        let mut state = versus(
            PlayerState {
                hand: vec![Card::Daze],
                battlefield: vec![Permanent::tapped(Card::Island)],
                ..PlayerState::default()
            },
            PlayerState::default(),
        );
        state.stack.push(their_spell(Card::Counterspell, None));
        let daze = Action::CastSpell {
            card: Card::Daze,
            payment: Payment {
                alt: Some(AltCost::ReturnIsland(Card::Island)),
                ..Payment::FREE
            },
            target: Some(on_stack(0, Card::Counterspell)),
            from: Zone::Hand,
        };
        assert!(legal_actions(&state).contains(&daze));
        let next = apply(&state, &daze).unwrap();
        assert_eq!(next.you.hand, vec![Card::Island]);
        assert!(next.you.battlefield.is_empty());
        // Daze resolves: the opponent has no mana, so they can only let
        // Counterspell be countered. The Island is then playable again with
        // the land drop.
        let taxed = pass_twice(&next);
        assert_eq!(taxed.pending.map(|p| p.player), Some(Player::Opponent));
        assert_eq!(
            legal_actions(&taxed),
            vec![Action::Choose(Choice::DeclineTax)]
        );
        let after = apply(&taxed, &Action::Choose(Choice::DeclineTax)).unwrap();
        assert!(after.stack.is_empty());
        assert_eq!(after.opponent.graveyard, vec![Card::Counterspell]);
        assert_eq!(after.you.graveyard, vec![Card::Daze]);
        assert!(legal_actions(&after).contains(&play_land_action(Card::Island)));
    }

    #[test]
    fn a_taxed_opponent_may_tap_lands_to_pay() {
        let mut state = versus(
            PlayerState {
                hand: vec![Card::SpellPierce],
                mana_pool: ManaPool::of(Color::U, 1),
                ..PlayerState::default()
            },
            opponent_with(Vec::new(), vec![Permanent::untapped(Card::Island); 2]),
        );
        state.stack.push(their_spell(Card::Counterspell, None));
        let pierce = Action::CastSpell {
            card: Card::SpellPierce,
            payment: Payment::mana(ManaPool::of(Color::U, 1)),
            target: Some(on_stack(0, Card::Counterspell)),
            from: Zone::Hand,
        };
        let taxed = pass_twice(&apply(&state, &pierce).unwrap());
        assert_eq!(
            legal_actions(&taxed),
            vec![
                Action::Choose(Choice::PayTax(ManaPool::of(Color::U, 2))),
                Action::Choose(Choice::DeclineTax),
            ]
        );
        let paid = apply(
            &taxed,
            &Action::Choose(Choice::PayTax(ManaPool::of(Color::U, 2))),
        )
        .unwrap();
        assert_eq!(
            paid.opponent.battlefield,
            vec![Permanent::tapped(Card::Island); 2]
        );
        assert!(paid.opponent.mana_pool.is_empty());
        assert_eq!(paid.stack, vec![their_spell(Card::Counterspell, None)]);
    }

    #[test]
    fn removing_a_stack_item_shifts_targets_above_it() {
        let mut state = main_phase(PlayerState::default());
        state.stack.push(your_spell(Card::Brainstorm));
        state.stack.push(your_spell(Card::DarkRitual));
        state
            .stack
            .push(their_spell(Card::Daze, Some(on_stack(1, Card::DarkRitual))));
        state.stack.push(their_spell(
            Card::Counterspell,
            Some(on_stack(0, Card::Brainstorm)),
        ));
        let removed = remove_stack_item(&mut state, 0);
        assert_eq!(removed, your_spell(Card::Brainstorm));
        assert_eq!(
            state.stack,
            vec![
                your_spell(Card::DarkRitual),
                their_spell(Card::Daze, Some(on_stack(0, Card::DarkRitual))),
                // Its target is gone: it will fizzle.
                their_spell(Card::Counterspell, None),
            ]
        );
    }
}
