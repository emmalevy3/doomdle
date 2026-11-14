//! Game state: zones, the stack, priority and the puzzle definition.

use std::fmt;

use crate::cards::{AbilityKind, Card, CardKind, LandFilter, TriggerKind};
use crate::engine::{Action, EngineError, apply};
use crate::mana::ManaPool;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Player {
    You,
    Opponent,
}

impl Player {
    pub fn other(self) -> Player {
        match self {
            Player::You => Player::Opponent,
            Player::Opponent => Player::You,
        }
    }
}

/// Where a card is cast or played from. Normally the hand; the graveyard
/// under Yawgmoth's Will.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Zone {
    Hand,
    Graveyard,
}

/// What a spell targets, fixed when it is cast.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Target {
    /// A card in the caster's graveyard (Unearth).
    Card(Card),
    /// An item on the stack: its index from the bottom, plus the card or
    /// ability source sitting there, so the action reads naturally and the
    /// engine can check it is still the item meant.
    Stack { index: usize, card: Card },
}

/// A card on the battlefield. Which player controls it is given by which
/// battlefield it sits on. `loyalty` and `loyalty_used` are only meaningful
/// for planeswalkers and stay 0/false otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Permanent {
    pub card: Card,
    pub tapped: bool,
    pub summoning_sick: bool,
    pub loyalty: u8,
    pub loyalty_used: bool,
}

impl Permanent {
    /// A permanent that has been around since before this turn: untapped, not
    /// summoning sick, a planeswalker at its starting loyalty.
    pub fn untapped(card: Card) -> Permanent {
        Permanent {
            card,
            tapped: false,
            summoning_sick: false,
            loyalty: card.starting_loyalty(),
            loyalty_used: false,
        }
    }

    pub fn tapped(card: Card) -> Permanent {
        Permanent {
            tapped: true,
            ..Permanent::untapped(card)
        }
    }

    /// A permanent as it enters the battlefield this turn.
    pub fn entering(card: Card, tapped: bool) -> Permanent {
        Permanent {
            tapped,
            summoning_sick: card.kind() == CardKind::Creature,
            ..Permanent::untapped(card)
        }
    }
}

/// Something waiting to resolve. The top of the stack is the last element.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StackItem {
    Spell {
        card: Card,
        controller: Player,
        /// The target chosen on casting. `None` for untargeted spells, and
        /// also for a targeted spell whose target has since left the stack:
        /// such a spell does nothing when it resolves.
        target: Option<Target>,
    },
    Trigger {
        source: Card,
        kind: TriggerKind,
        controller: Player,
    },
    /// An activated ability whose costs are already paid.
    Ability {
        source: Card,
        kind: AbilityKind,
        controller: Player,
    },
}

impl StackItem {
    pub fn controller(&self) -> Player {
        match self {
            StackItem::Spell { controller, .. }
            | StackItem::Trigger { controller, .. }
            | StackItem::Ability { controller, .. } => *controller,
        }
    }

    /// The spell's card, or the source of a trigger or ability.
    pub fn card(&self) -> Card {
        match self {
            StackItem::Spell { card, .. } => *card,
            StackItem::Trigger { source, .. } | StackItem::Ability { source, .. } => *source,
        }
    }
}

/// Which decision a paused resolution is waiting on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChoiceKind {
    /// Brainstorm has drawn three; pick two cards from hand to put back.
    BrainstormPutBack,
    /// Ponder: reorder the top (up to) three cards.
    PonderOrder,
    /// Preordain: scry 2.
    PreordainScry,
    /// Consider: surveil 1, then draw.
    ConsiderSurveil,
    /// Surveil 1 on its own (Undercity Sewers, Meticulous Archive).
    Surveil,
    /// Search the library for a land matching the filter, or find nothing.
    SearchLand(LandFilter),
    /// Flow State: the top (up to) three cards are being looked at; put
    /// `take` of them into hand and the rest on the bottom in any order.
    FlowState { take: usize },
    /// Frantic Search has drawn two; discard two (or the whole hand if
    /// smaller).
    FranticDiscard,
    /// Frantic Search: untap up to three lands.
    UntapLands,
    /// "Counter target spell unless its controller pays {amount}" (Daze,
    /// Spell Pierce, Miscalculation): the target's controller decides.
    PayTax { amount: u8, target: Target },
}

/// A resolution that stopped for a player decision. Nobody gets priority
/// until the decision is made; `bin` is the spell being resolved, if any, and
/// goes to `bin_owner`'s graveyard once the choice is applied (the opponent's
/// Daze is binned while *you* decide whether to pay).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PendingChoice {
    pub player: Player,
    pub kind: ChoiceKind,
    pub bin: Option<Card>,
    pub bin_owner: Player,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LossReason {
    /// Both players passed on an empty stack: the turn ends without a win.
    TurnEnded,
    DrewFromEmptyLibrary,
    LifeZero,
    /// A step depended on the order of a library that is no longer known
    /// (CARDS.md, "Shuffles").
    LibraryUnordered,
}

/// The game result from your point of view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Outcome {
    Win,
    Loss(LossReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phase {
    /// Doomsday is resolving: choose and order the pile.
    ChoosingPile,
    /// Your precombat main phase.
    Main,
}

/// Everything one player owns or has done this turn.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlayerState {
    pub life: i16,
    /// Kept sorted (a multiset).
    pub hand: Vec<Card>,
    /// Index 0 is the top. In a `Puzzle` this is the unordered set of cards
    /// the pile may be built from.
    pub library: Vec<Card>,
    /// False after a shuffle or random placement; see CARDS.md "Shuffles".
    pub library_ordered: bool,
    /// Kept sorted.
    pub graveyard: Vec<Card>,
    /// Kept sorted.
    pub exile: Vec<Card>,
    /// Kept sorted.
    pub battlefield: Vec<Permanent>,
    pub mana_pool: ManaPool,
    /// Spells cast this turn (the storm count). In a `Puzzle` this is the
    /// count *before* Doomsday; the engine adds Doomsday itself when the pile
    /// is chosen.
    pub spells_cast_this_turn: u8,
    /// Cards drawn this turn, including the draw step.
    pub cards_drawn_this_turn: u8,
    pub land_drop_available: bool,
    /// Set when a draw found no card and nothing replaced it; the loss is
    /// applied by the next state-based check, so a win in the same resolution
    /// (Doctor Doom) still counts.
    pub drew_from_empty: bool,
    /// Yawgmoth's Will has resolved this turn: spells and lands may be cast or
    /// played from the graveyard, and anything that would go to the graveyard
    /// is exiled instead.
    pub yawgmoths_will: bool,
}

impl Default for PlayerState {
    fn default() -> PlayerState {
        PlayerState {
            life: 20,
            hand: Vec::new(),
            library: Vec::new(),
            library_ordered: true,
            graveyard: Vec::new(),
            exile: Vec::new(),
            battlefield: Vec::new(),
            mana_pool: ManaPool::EMPTY,
            spells_cast_this_turn: 0,
            cards_drawn_this_turn: 0,
            land_drop_available: true,
            drew_from_empty: false,
            yawgmoths_will: false,
        }
    }
}

impl PlayerState {
    /// Whether an effect may look at or draw the top `n` cards. An unordered
    /// library is still fine when the effect covers all of it.
    pub fn can_see_top(&self, n: usize) -> bool {
        self.library_ordered || n >= self.library.len()
    }

    /// Devotion to blue: {U} pips in the mana costs of permanents controlled.
    pub fn blue_devotion(&self) -> u8 {
        self.battlefield
            .iter()
            .map(|p| p.card.blue_devotion())
            .sum()
    }

    /// Lands controlled (Edge of Autumn's "four or fewer lands").
    pub fn land_count(&self) -> usize {
        self.battlefield
            .iter()
            .filter(|p| p.card.kind() == CardKind::Land)
            .count()
    }

    /// Laboratory Maniac or Jace turns a draw from an empty library into a win.
    fn wins_on_empty_draw(&self) -> bool {
        self.battlefield
            .iter()
            .any(|p| p.card.replaces_empty_draw_with_win())
    }

    /// Removes one copy of `card` from hand.
    pub(crate) fn remove_from_hand(&mut self, card: Card) -> Option<Card> {
        let pos = self.hand.iter().position(|&c| c == card)?;
        Some(self.hand.remove(pos))
    }

    /// The one way a card reaches the graveyard, so Yawgmoth's Will's
    /// "exile it instead" applies everywhere: resolved spells, discards,
    /// sacrifices, mills and surveils alike.
    pub(crate) fn put_in_graveyard(&mut self, card: Card) {
        if self.yawgmoths_will {
            self.exile.push(card);
        } else {
            self.graveyard.push(card);
        }
    }

    /// Removes one copy of `card` from the graveyard.
    pub(crate) fn remove_from_graveyard(&mut self, card: Card) -> Option<Card> {
        let pos = self.graveyard.iter().position(|&c| c == card)?;
        Some(self.graveyard.remove(pos))
    }

    /// Whether the graveyard holds a card of this kind (Flow State).
    pub fn graveyard_has(&self, kind: CardKind) -> bool {
        self.graveyard.iter().any(|c| c.kind() == kind)
    }

    /// Mills the top `n` cards (fewer if the library is smaller).
    pub(crate) fn mill(&mut self, n: usize) {
        let k = n.min(self.library.len());
        let milled: Vec<Card> = self.library.drain(..k).collect();
        for card in milled {
            self.put_in_graveyard(card);
        }
    }

    /// The library was shuffled: its order is unknown unless at most one card
    /// is left.
    pub(crate) fn shuffle(&mut self) {
        if self.library.len() >= 2 {
            self.library_ordered = false;
        }
    }

    fn normalize(&mut self) {
        self.hand.sort();
        self.graveyard.sort();
        self.exile.sort();
        self.battlefield.sort();
    }
}

/// A puzzle as Doomsday resolves. Doomsday itself is on the stack, so it is
/// not listed in any zone; the engine puts it in the graveyard when the pile
/// is chosen. `you.library` holds the candidate cards in no particular order,
/// and `you.life` is the total *before* Doomsday's life loss.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Puzzle {
    pub you: PlayerState,
    pub opponent: PlayerState,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct State {
    pub phase: Phase,
    pub priority: Player,
    /// True when the previous action was the other player passing, so a pass
    /// now resolves the top of the stack (or ends the turn).
    pub passed_in_succession: bool,
    pub stack: Vec<StackItem>,
    pub pending: Option<PendingChoice>,
    pub you: PlayerState,
    pub opponent: PlayerState,
    pub outcome: Option<Outcome>,
}

impl State {
    /// The state as Doomsday resolves, before the pile is chosen.
    pub fn new(puzzle: Puzzle) -> State {
        let mut state = State {
            phase: Phase::ChoosingPile,
            priority: Player::You,
            passed_in_succession: false,
            stack: Vec::new(),
            pending: None,
            you: puzzle.you,
            opponent: puzzle.opponent,
            outcome: None,
        };
        state.normalize();
        state
    }

    /// The state right after Doomsday resolves with the given pile (top first).
    pub fn with_pile(puzzle: Puzzle, pile: &[Card]) -> Result<State, EngineError> {
        apply(&State::new(puzzle), &Action::ChoosePile(pile.to_vec()))
    }

    pub fn player(&self, player: Player) -> &PlayerState {
        match player {
            Player::You => &self.you,
            Player::Opponent => &self.opponent,
        }
    }

    pub fn player_mut(&mut self, player: Player) -> &mut PlayerState {
        match player {
            Player::You => &mut self.you,
            Player::Opponent => &mut self.opponent,
        }
    }

    /// Who has to decide next, or `None` once the game is over.
    pub fn actor(&self) -> Option<Player> {
        if self.outcome.is_some() {
            return None;
        }
        if let Some(pending) = &self.pending {
            return Some(pending.player);
        }
        match self.phase {
            Phase::ChoosingPile => Some(Player::You),
            Phase::Main => Some(self.priority),
        }
    }

    /// Records a loss unless the game already has an outcome ("you win the
    /// game" effects are immediate and beat a later state-based loss).
    pub(crate) fn lose(&mut self, reason: LossReason) {
        if self.outcome.is_none() {
            self.outcome = Some(Outcome::Loss(reason));
        }
    }

    /// Records your win.
    pub(crate) fn win(&mut self, player: Player) {
        if player == Player::You && self.outcome.is_none() {
            self.outcome = Some(Outcome::Win);
        }
    }

    /// One draw instruction of `n` cards. A draw from an empty library is a
    /// win with Laboratory Maniac or Jace in play, and otherwise a loss at the
    /// next state-based check.
    pub(crate) fn draw(&mut self, player: Player, n: usize) {
        if self.outcome.is_some() {
            return;
        }
        if !self.player(player).can_see_top(n) {
            self.lose(LossReason::LibraryUnordered);
            return;
        }
        for _ in 0..n {
            let ps = self.player_mut(player);
            if ps.library.is_empty() {
                if ps.wins_on_empty_draw() {
                    self.win(player);
                } else {
                    ps.drew_from_empty = true;
                }
                return;
            }
            let card = ps.library.remove(0);
            ps.hand.push(card);
            ps.cards_drawn_this_turn += 1;
        }
    }

    /// Puts `card` onto `player`'s battlefield and queues its enter trigger.
    pub(crate) fn enter_battlefield(&mut self, player: Player, card: Card, tapped: bool) {
        self.player_mut(player)
            .battlefield
            .push(Permanent::entering(card, tapped));
        if let Some(kind) = card.etb_trigger() {
            self.stack.push(StackItem::Trigger {
                source: card,
                kind,
                controller: player,
            });
        }
    }

    /// State-based checks, run after every action.
    pub(crate) fn check_state_based(&mut self) {
        if self.you.life <= 0 {
            self.lose(LossReason::LifeZero);
        }
        // The opponent only loses life by paying it (Force of Will, Phyrexian
        // mana); at 0 they lose.
        if self.opponent.life <= 0 {
            self.win(Player::You);
        }
        if self.you.drew_from_empty {
            self.lose(LossReason::DrewFromEmptyLibrary);
        }
    }

    /// Canonical form so equivalent states compare and hash equal.
    pub(crate) fn normalize(&mut self) {
        self.you.normalize();
        self.opponent.normalize();
    }
}

fn list_cards(cards: &[Card]) -> String {
    if cards.is_empty() {
        return "-".to_string();
    }
    cards
        .iter()
        .map(|c| c.name())
        .collect::<Vec<_>>()
        .join(", ")
}

impl fmt::Display for Permanent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.card.name())?;
        if self.tapped {
            f.write_str(" (tapped)")?;
        }
        if self.summoning_sick {
            f.write_str(" (summoning sick)")?;
        }
        if self.card.kind() == CardKind::Planeswalker {
            write!(f, " (loyalty {})", self.loyalty)?;
            if self.loyalty_used {
                f.write_str(" (activated)")?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Card(card) | Target::Stack { card, .. } => write!(f, "{card}"),
        }
    }
}

impl fmt::Display for StackItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StackItem::Spell {
                card,
                controller,
                target,
            } => {
                write!(f, "{card}")?;
                if let Some(target) = target {
                    write!(f, " targeting {target}")?;
                }
                write!(f, " ({controller:?})")
            }
            StackItem::Trigger {
                source, controller, ..
            } => write!(f, "{source} trigger ({controller:?})"),
            StackItem::Ability {
                source,
                kind,
                controller,
            } => write!(f, "{source} {kind} ({controller:?})"),
        }
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Outcome::Win => f.write_str("you win"),
            Outcome::Loss(LossReason::TurnEnded) => f.write_str("the turn ends without a win"),
            Outcome::Loss(LossReason::DrewFromEmptyLibrary) => {
                f.write_str("you drew from an empty library")
            }
            Outcome::Loss(LossReason::LifeZero) => f.write_str("your life total hit 0"),
            Outcome::Loss(LossReason::LibraryUnordered) => {
                f.write_str("the line depended on the order of a shuffled library")
            }
        }
    }
}

impl fmt::Display for PlayerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  life {}, mana pool {}", self.life, self.mana_pool)?;
        if self.yawgmoths_will {
            writeln!(f, "  Yawgmoth's Will active")?;
        }
        writeln!(f, "  hand: {}", list_cards(&self.hand))?;
        let battlefield = if self.battlefield.is_empty() {
            "-".to_string()
        } else {
            self.battlefield
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        writeln!(f, "  battlefield: {battlefield}")?;
        let order = if self.library_ordered {
            "top first"
        } else {
            "order unknown"
        };
        writeln!(f, "  library ({order}): {}", list_cards(&self.library))?;
        write!(f, "  graveyard: {}", list_cards(&self.graveyard))
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.phase {
            Phase::ChoosingPile => writeln!(f, "Doomsday is resolving: choose the pile")?,
            Phase::Main => {
                if let Some(outcome) = self.outcome {
                    writeln!(f, "Game over: {outcome}")?;
                } else if let Some(pending) = &self.pending {
                    writeln!(f, "{:?} must decide: {:?}", pending.player, pending.kind)?;
                } else {
                    writeln!(f, "{:?} has priority", self.priority)?;
                }
            }
        }
        if self.stack.is_empty() {
            writeln!(f, "Stack: empty")?;
        } else {
            let items: Vec<String> = self.stack.iter().rev().map(|i| i.to_string()).collect();
            writeln!(f, "Stack (top first): {}", items.join(", "))?;
        }
        writeln!(f, "You:")?;
        writeln!(f, "{}", self.you)?;
        writeln!(f, "Opponent:")?;
        write!(f, "{}", self.opponent)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// A main-phase state with you holding priority and an empty stack.
    pub(crate) fn main_phase(you: PlayerState) -> State {
        versus(you, PlayerState::default())
    }

    /// A main-phase state against a real opponent; you hold priority.
    pub(crate) fn versus(you: PlayerState, opponent: PlayerState) -> State {
        let mut state = State {
            phase: Phase::Main,
            priority: Player::You,
            passed_in_succession: false,
            stack: Vec::new(),
            pending: None,
            you,
            opponent,
            outcome: None,
        };
        state.normalize();
        state
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::main_phase;
    use super::*;

    #[test]
    fn draw_moves_top_card_and_counts() {
        let mut state = main_phase(PlayerState {
            library: vec![Card::Brainstorm, Card::Island],
            ..PlayerState::default()
        });
        state.draw(Player::You, 1);
        assert_eq!(state.you.hand, vec![Card::Brainstorm]);
        assert_eq!(state.you.library, vec![Card::Island]);
        assert_eq!(state.you.cards_drawn_this_turn, 1);
        assert_eq!(state.outcome, None);
    }

    #[test]
    fn draw_from_empty_library_loses_at_the_state_based_check() {
        let mut state = main_phase(PlayerState {
            library: vec![Card::Island],
            ..PlayerState::default()
        });
        state.draw(Player::You, 2);
        assert!(state.you.drew_from_empty);
        assert_eq!(state.outcome, None, "the loss waits for the check");
        state.check_state_based();
        assert_eq!(
            state.outcome,
            Some(Outcome::Loss(LossReason::DrewFromEmptyLibrary))
        );
    }

    #[test]
    fn lab_man_and_jace_turn_an_empty_draw_into_a_win() {
        for win_con in [Card::LaboratoryManiac, Card::JaceWielderOfMysteries] {
            let mut state = main_phase(PlayerState {
                battlefield: vec![Permanent::untapped(win_con)],
                ..PlayerState::default()
            });
            state.draw(Player::You, 1);
            assert_eq!(state.outcome, Some(Outcome::Win), "{win_con}");
            assert!(!state.you.drew_from_empty);
        }
        // Thassa's Oracle is not a replacement effect.
        let mut state = main_phase(PlayerState {
            battlefield: vec![Permanent::untapped(Card::ThassasOracle)],
            ..PlayerState::default()
        });
        state.draw(Player::You, 1);
        assert_eq!(state.outcome, None);
        assert!(state.you.drew_from_empty);
    }

    #[test]
    fn draw_from_unordered_library_fails_unless_it_takes_everything() {
        let mut state = main_phase(PlayerState {
            library: vec![Card::Island, Card::Swamp],
            library_ordered: false,
            ..PlayerState::default()
        });
        let mut partial = state.clone();
        partial.draw(Player::You, 1);
        assert_eq!(
            partial.outcome,
            Some(Outcome::Loss(LossReason::LibraryUnordered))
        );
        state.draw(Player::You, 2);
        assert_eq!(state.outcome, None);
        assert_eq!(state.you.hand, vec![Card::Island, Card::Swamp]);
    }

    #[test]
    fn win_is_not_overwritten_by_a_later_loss() {
        let mut state = main_phase(PlayerState::default());
        state.outcome = Some(Outcome::Win);
        state.lose(LossReason::LifeZero);
        state.you.drew_from_empty = true;
        state.you.life = 0;
        state.check_state_based();
        assert_eq!(state.outcome, Some(Outcome::Win));
    }

    #[test]
    fn devotion_counts_blue_pips_on_your_permanents() {
        let state = main_phase(PlayerState {
            battlefield: vec![
                Permanent::untapped(Card::ThassasOracle),
                Permanent::untapped(Card::MoxSapphire),
                Permanent::untapped(Card::Island),
                Permanent::untapped(Card::JaceWielderOfMysteries),
                Permanent::untapped(Card::LaboratoryManiac),
                Permanent::untapped(Card::DoctorDoom),
            ],
            ..PlayerState::default()
        });
        assert_eq!(state.you.blue_devotion(), 2 + 3 + 1);
    }

    #[test]
    fn shuffle_keeps_order_with_at_most_one_card() {
        let mut one = PlayerState {
            library: vec![Card::Island],
            ..PlayerState::default()
        };
        one.shuffle();
        assert!(one.library_ordered);
        let mut two = PlayerState {
            library: vec![Card::Island, Card::Swamp],
            ..PlayerState::default()
        };
        two.shuffle();
        assert!(!two.library_ordered);
    }

    #[test]
    fn mill_moves_top_cards_to_graveyard() {
        let mut ps = PlayerState {
            library: vec![Card::Island, Card::Swamp, Card::Plains],
            ..PlayerState::default()
        };
        ps.mill(2);
        assert_eq!(ps.library, vec![Card::Plains]);
        assert_eq!(ps.graveyard, vec![Card::Island, Card::Swamp]);
        ps.mill(5);
        assert!(ps.library.is_empty());
    }

    #[test]
    fn yawgmoths_will_sends_graveyard_bound_cards_to_exile() {
        let mut ps = PlayerState {
            library: vec![Card::Island, Card::Swamp],
            graveyard: vec![Card::Doomsday],
            ..PlayerState::default()
        };
        assert!(!PlayerState::default().yawgmoths_will);
        ps.put_in_graveyard(Card::Brainstorm);
        assert_eq!(ps.graveyard, vec![Card::Doomsday, Card::Brainstorm]);

        ps.yawgmoths_will = true;
        ps.put_in_graveyard(Card::Ponder);
        ps.mill(1);
        assert_eq!(ps.graveyard, vec![Card::Doomsday, Card::Brainstorm]);
        assert_eq!(ps.exile, vec![Card::Ponder, Card::Island]);
        assert_eq!(ps.library, vec![Card::Swamp]);
    }

    #[test]
    fn graveyard_has_checks_card_kinds() {
        let ps = PlayerState {
            graveyard: vec![Card::Doomsday, Card::LotusPetal],
            ..PlayerState::default()
        };
        assert!(ps.graveyard_has(CardKind::Sorcery));
        assert!(ps.graveyard_has(CardKind::Artifact));
        assert!(!ps.graveyard_has(CardKind::Instant));
    }

    #[test]
    fn entering_permanents_get_sickness_and_loyalty() {
        assert!(Permanent::entering(Card::ThassasOracle, false).summoning_sick);
        assert!(!Permanent::entering(Card::Island, true).summoning_sick);
        assert!(Permanent::entering(Card::Island, true).tapped);
        assert_eq!(
            Permanent::entering(Card::JaceWielderOfMysteries, false).loyalty,
            4
        );
        assert_eq!(Permanent::untapped(Card::JaceWielderOfMysteries).loyalty, 4);
    }

    #[test]
    fn normalize_makes_zone_order_irrelevant() {
        let a = main_phase(PlayerState {
            hand: vec![Card::Island, Card::Brainstorm],
            battlefield: vec![
                Permanent::untapped(Card::Swamp),
                Permanent::tapped(Card::Island),
            ],
            ..PlayerState::default()
        });
        let b = main_phase(PlayerState {
            hand: vec![Card::Brainstorm, Card::Island],
            battlefield: vec![
                Permanent::tapped(Card::Island),
                Permanent::untapped(Card::Swamp),
            ],
            ..PlayerState::default()
        });
        assert_eq!(a, b);
    }
}
