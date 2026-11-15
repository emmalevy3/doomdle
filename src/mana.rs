//! Mana as simple per-color counts (plus colorless), per CLAUDE.md, and the
//! payments (mana plus life) a player can make for a cost.

use std::fmt;

use crate::cards::Card;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Color {
    W,
    U,
    B,
    R,
    G,
}

impl Color {
    pub const ALL: [Color; 5] = [Color::W, Color::U, Color::B, Color::R, Color::G];

    const fn index(self) -> usize {
        match self {
            Color::W => 0,
            Color::U => 1,
            Color::B => 2,
            Color::R => 3,
            Color::G => 4,
        }
    }

    pub const fn symbol(self) -> char {
        match self {
            Color::W => 'W',
            Color::U => 'U',
            Color::B => 'B',
            Color::R => 'R',
            Color::G => 'G',
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.symbol())
    }
}

/// Index of colorless mana in [`ManaPool::amounts`].
const COLORLESS: usize = 5;

/// Mana available to spend: five colors plus colorless. Also used to describe
/// the mana part of a payment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ManaPool {
    amounts: [u8; 6],
}

impl ManaPool {
    pub const EMPTY: ManaPool = ManaPool { amounts: [0; 6] };

    /// `n` mana of a single color.
    pub const fn of(color: Color, n: u8) -> ManaPool {
        let mut amounts = [0; 6];
        amounts[color.index()] = n;
        ManaPool { amounts }
    }

    /// `n` colorless mana.
    pub const fn colorless(n: u8) -> ManaPool {
        let mut amounts = [0; 6];
        amounts[COLORLESS] = n;
        ManaPool { amounts }
    }

    /// Builder-style: this pool plus `n` more of `color`.
    /// Builder-style: this pool plus `n` colorless mana.
    pub fn with_colorless(mut self, n: u8) -> ManaPool {
        self.amounts[COLORLESS] += n;
        self
    }

    pub fn with(mut self, color: Color, n: u8) -> ManaPool {
        self.add_mana(color, n);
        self
    }

    pub fn get(&self, color: Color) -> u8 {
        self.amounts[color.index()]
    }

    pub fn get_colorless(&self) -> u8 {
        self.amounts[COLORLESS]
    }

    pub fn add_mana(&mut self, color: Color, n: u8) {
        self.amounts[color.index()] += n;
    }

    pub fn add_colorless(&mut self, n: u8) {
        self.amounts[COLORLESS] += n;
    }

    pub fn total(&self) -> u8 {
        self.amounts.iter().sum()
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }

    pub fn plus(&self, other: &ManaPool) -> ManaPool {
        let mut amounts = self.amounts;
        for (a, b) in amounts.iter_mut().zip(other.amounts) {
            *a += b;
        }
        ManaPool { amounts }
    }

    /// `self - other`, or `None` if any component would go negative.
    pub fn minus(&self, other: &ManaPool) -> Option<ManaPool> {
        let mut amounts = self.amounts;
        for (a, b) in amounts.iter_mut().zip(other.amounts) {
            *a = a.checked_sub(b)?;
        }
        Some(ManaPool { amounts })
    }

    /// Every distinct way to pay the mana of `cost` out of this pool, treating
    /// Phyrexian pips as their colored half. Colored pips must be paid
    /// exactly; generic mana can come from anything left over. Empty if the
    /// pool can't cover the cost. See [`Payment::options`] for life payments.
    pub fn payments_for(&self, cost: &ManaCost) -> Vec<ManaPool> {
        // Each {R/G} pip is paid with red or green: try every split.
        if cost.hybrid > 0 {
            let mut out = Vec::new();
            for red in 0..=cost.hybrid {
                for payment in self.payments_for(&cost.with_hybrid_as(red)) {
                    if !out.contains(&payment) {
                        out.push(payment);
                    }
                }
            }
            return out;
        }
        let pips = cost.pips_as_pool();
        let Some(spare) = self.minus(&pips) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut acc = pips;
        distribute_generic(&spare.amounts, 0, cost.generic, &mut acc, &mut out);
        out
    }
}

/// Recursively assign `remaining` generic mana to kinds `idx..`, bounded by
/// what is `spare` of each kind.
fn distribute_generic(
    spare: &[u8; 6],
    idx: usize,
    remaining: u8,
    acc: &mut ManaPool,
    out: &mut Vec<ManaPool>,
) {
    if remaining == 0 {
        out.push(*acc);
        return;
    }
    if idx == spare.len() {
        return;
    }
    let max_take = spare[idx].min(remaining);
    for take in 0..=max_take {
        acc.amounts[idx] += take;
        distribute_generic(spare, idx + 1, remaining - take, acc, out);
        acc.amounts[idx] -= take;
    }
}

impl fmt::Display for ManaPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return write!(f, "0");
        }
        for color in Color::ALL {
            for _ in 0..self.get(color) {
                write!(f, "{}", color.symbol())?;
            }
        }
        for _ in 0..self.get_colorless() {
            write!(f, "C")?;
        }
        Ok(())
    }
}

/// A mana cost: colored pips, Phyrexian pips (pay the color or 2 life),
/// {R/G} hybrid pips (the only hybrid in the pool, Manamorphose) and a
/// generic amount.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ManaCost {
    pips: [u8; 5],
    phyrexian: [u8; 5],
    /// {R/G} pips: each is paid with red or green.
    pub hybrid: u8,
    pub generic: u8,
}

impl ManaCost {
    pub const FREE: ManaCost = ManaCost {
        pips: [0; 5],
        phyrexian: [0; 5],
        hybrid: 0,
        generic: 0,
    };

    /// `n` pips of one color and nothing else.
    pub const fn colored(color: Color, n: u8) -> ManaCost {
        let mut pips = [0; 5];
        pips[color.index()] = n;
        ManaCost {
            pips,
            ..ManaCost::FREE
        }
    }

    /// `n` Phyrexian pips of one color and nothing else.
    pub const fn phyrexian(color: Color, n: u8) -> ManaCost {
        let mut phyrexian = [0; 5];
        phyrexian[color.index()] = n;
        ManaCost {
            phyrexian,
            ..ManaCost::FREE
        }
    }

    /// Builder-style: this cost plus `n` more pips of `color`.
    pub fn with(mut self, color: Color, n: u8) -> ManaCost {
        self.pips[color.index()] += n;
        self
    }

    /// Builder-style: this cost plus `n` generic mana.
    pub fn with_generic(mut self, n: u8) -> ManaCost {
        self.generic += n;
        self
    }

    /// Builder-style: this cost plus `n` {R/G} pips.
    pub fn with_hybrid(mut self, n: u8) -> ManaCost {
        self.hybrid += n;
        self
    }

    /// Colored (non-Phyrexian) pips of `color`.
    pub fn pip(&self, color: Color) -> u8 {
        self.pips[color.index()]
    }

    pub fn phyrexian_pip(&self, color: Color) -> u8 {
        self.phyrexian[color.index()]
    }

    /// Phyrexian and hybrid pips count as one each, whatever is paid for them.
    pub fn mana_value(&self) -> u8 {
        self.pips.iter().sum::<u8>()
            + self.phyrexian.iter().sum::<u8>()
            + self.hybrid
            + self.generic
    }

    /// This cost with its hybrid pips settled: `red` of them paid with red
    /// mana and the rest with green.
    fn with_hybrid_as(&self, red: u8) -> ManaCost {
        let mut cost = *self;
        cost.pips[Color::R.index()] += red;
        cost.pips[Color::G.index()] += self.hybrid - red;
        cost.hybrid = 0;
        cost
    }

    /// Colored pips plus Phyrexian pips, as a pool: what paying everything
    /// with mana costs.
    fn pips_as_pool(&self) -> ManaPool {
        let mut pool = ManaPool::EMPTY;
        for color in Color::ALL {
            pool.add_mana(color, self.pip(color) + self.phyrexian_pip(color));
        }
        pool
    }

    /// This cost with `life_paid[c]` Phyrexian pips of each color paid with
    /// life instead of mana: those pips are dropped, the rest become colored.
    fn with_phyrexian_paid_by_life(&self, life_paid: [u8; 5]) -> ManaCost {
        let mut cost = *self;
        for color in Color::ALL {
            let i = color.index();
            cost.pips[i] += cost.phyrexian[i] - life_paid[i];
            cost.phyrexian[i] = 0;
        }
        cost
    }
}

impl fmt::Display for ManaCost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mana_value() == 0 {
            return write!(f, "{{0}}");
        }
        if self.generic > 0 {
            write!(f, "{{{}}}", self.generic)?;
        }
        for color in Color::ALL {
            for _ in 0..self.pip(color) {
                write!(f, "{{{}}}", color.symbol())?;
            }
        }
        for color in Color::ALL {
            for _ in 0..self.phyrexian_pip(color) {
                write!(f, "{{{}/P}}", color.symbol())?;
            }
        }
        for _ in 0..self.hybrid {
            write!(f, "{{R/G}}")?;
        }
        Ok(())
    }
}

/// The non-mana part of an alternative cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AltCost {
    /// Exile this card from hand (Force of Will, Force of Negation, evoke).
    Pitch(Card),
    /// Return this Island you control to hand (Daze).
    ReturnIsland(Card),
    /// Return these two Islands you control to hand (Gush); sorted.
    ReturnTwoIslands(Card, Card),
    /// Warp (Quantum Riddler): the card's warp cost instead of its mana cost.
    Warp,
}

/// What a player hands over for a spell or ability: mana from the pool, life
/// (Phyrexian mana, Street Wraith's cycling, Force of Will), for an
/// alternative cost the card it pitches or bounces, how many times a
/// replicate cost was paid (Consign to Memory; the mana for it is included
/// in `mana`), whether offspring was paid (Thundertrap Trainer; included in
/// `mana` too), and how much of `mana` comes from Caverns of Souls tapped as
/// part of the cast rather than from the pool.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Payment {
    pub mana: ManaPool,
    pub life: u8,
    pub alt: Option<AltCost>,
    pub replicate: u8,
    pub offspring: bool,
    pub cavern: u8,
}

impl Payment {
    pub const FREE: Payment = Payment {
        mana: ManaPool::EMPTY,
        life: 0,
        alt: None,
        replicate: 0,
        offspring: false,
        cavern: 0,
    };

    pub const fn mana(mana: ManaPool) -> Payment {
        Payment {
            mana,
            ..Payment::FREE
        }
    }

    pub const fn life(life: u8) -> Payment {
        Payment {
            life,
            ..Payment::FREE
        }
    }

    pub fn is_free(&self) -> bool {
        self.mana.is_empty()
            && self.life == 0
            && self.alt.is_none()
            && self.replicate == 0
            && !self.offspring
    }

    /// Every distinct way to pay `cost` from `pool` with `life` to spare. Each
    /// Phyrexian pip is paid with its color or with 2 life; life can only be
    /// paid if the total is at most `life` (rule 119.4).
    pub fn options(pool: &ManaPool, cost: &ManaCost, life: i16) -> Vec<Payment> {
        let mut out = Vec::new();
        let mut life_paid = [0u8; 5];
        phyrexian_splits(cost, 0, &mut life_paid, &mut |split| {
            let life_cost = 2 * split.iter().sum::<u8>();
            if i16::from(life_cost) > life {
                return;
            }
            let mana_cost = cost.with_phyrexian_paid_by_life(split);
            for mana in pool.payments_for(&mana_cost) {
                let payment = Payment {
                    mana,
                    life: life_cost,
                    ..Payment::FREE
                };
                if !out.contains(&payment) {
                    out.push(payment);
                }
            }
        });
        out
    }
}

/// Calls `visit` with every choice of how many Phyrexian pips of each color
/// are paid with life.
fn phyrexian_splits(
    cost: &ManaCost,
    idx: usize,
    life_paid: &mut [u8; 5],
    visit: &mut impl FnMut([u8; 5]),
) {
    if idx == life_paid.len() {
        visit(*life_paid);
        return;
    }
    for n in 0..=cost.phyrexian[idx] {
        life_paid[idx] = n;
        phyrexian_splits(cost, idx + 1, life_paid, visit);
    }
    life_paid[idx] = 0;
}

impl From<ManaPool> for Payment {
    fn from(mana: ManaPool) -> Payment {
        Payment::mana(mana)
    }
}

impl fmt::Display for Payment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if !self.mana.is_empty() {
            parts.push(self.mana.to_string());
        }
        match self.cavern {
            0 => {}
            1 => parts.push("1 of it from Cavern of Souls".to_string()),
            n => parts.push(format!("{n} of it from Caverns of Souls")),
        }
        if self.life > 0 {
            parts.push(format!("{} life", self.life));
        }
        match self.alt {
            Some(AltCost::Pitch(card)) => parts.push(format!("exiling {card} from hand")),
            Some(AltCost::ReturnIsland(card)) => parts.push(format!("returning {card} to hand")),
            Some(AltCost::ReturnTwoIslands(a, b)) => {
                parts.push(format!("returning {a} and {b} to hand"))
            }
            Some(AltCost::Warp) => parts.push("warped".to_string()),
            None => {}
        }
        match self.replicate {
            0 => {}
            1 => parts.push("replicated once".to_string()),
            n => parts.push(format!("replicated {n} times")),
        }
        if self.offspring {
            parts.push("with offspring".to_string());
        }
        if parts.is_empty() {
            f.write_str("nothing")
        } else {
            f.write_str(&parts.join(" and "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_colored_payment() {
        let pool = ManaPool::of(Color::U, 2).with(Color::B, 1);
        let cost = ManaCost::colored(Color::U, 2);
        assert_eq!(pool.payments_for(&cost), vec![ManaPool::of(Color::U, 2)]);
    }

    #[test]
    fn no_payment_when_short() {
        let pool = ManaPool::of(Color::U, 1);
        assert!(
            pool.payments_for(&ManaCost::colored(Color::U, 2))
                .is_empty()
        );
        assert!(
            ManaPool::of(Color::B, 5)
                .payments_for(&ManaCost::colored(Color::U, 1))
                .is_empty()
        );
    }

    #[test]
    fn free_cost_has_one_empty_payment() {
        assert_eq!(
            ManaPool::EMPTY.payments_for(&ManaCost::FREE),
            vec![ManaPool::EMPTY]
        );
        assert_eq!(
            ManaPool::of(Color::U, 3).payments_for(&ManaCost::FREE),
            vec![ManaPool::EMPTY]
        );
    }

    #[test]
    fn generic_enumerates_every_split() {
        // {1}{U} from UUB: generic can be the spare U or the B.
        let pool = ManaPool::of(Color::U, 2).with(Color::B, 1);
        let cost = ManaCost::colored(Color::U, 1).with_generic(1);
        let mut payments = pool.payments_for(&cost);
        payments.sort();
        let mut expected = vec![
            ManaPool::of(Color::U, 2),
            ManaPool::of(Color::U, 1).with(Color::B, 1),
        ];
        expected.sort();
        assert_eq!(payments, expected);
    }

    #[test]
    fn generic_can_use_colorless() {
        let pool = ManaPool::of(Color::B, 2).plus(&ManaPool::colorless(2));
        let cost = ManaCost::colored(Color::B, 2).with_generic(2);
        assert_eq!(
            pool.payments_for(&cost),
            vec![ManaPool::of(Color::B, 2).plus(&ManaPool::colorless(2))]
        );
    }

    #[test]
    fn minus_and_plus() {
        let a = ManaPool::of(Color::U, 3);
        let b = ManaPool::of(Color::U, 1);
        assert_eq!(a.minus(&b), Some(ManaPool::of(Color::U, 2)));
        assert_eq!(b.minus(&a), None);
        assert_eq!(a.plus(&b), ManaPool::of(Color::U, 4));
    }

    #[test]
    fn phyrexian_pip_is_paid_with_color_or_two_life() {
        let probe = ManaCost::phyrexian(Color::U, 1);
        assert_eq!(probe.mana_value(), 1);
        let mut options = Payment::options(&ManaPool::of(Color::U, 1), &probe, 10);
        options.sort();
        let mut expected = vec![Payment::mana(ManaPool::of(Color::U, 1)), Payment::life(2)];
        expected.sort();
        assert_eq!(options, expected);
        // No blue: life only. Life 1: nothing at all.
        assert_eq!(
            Payment::options(&ManaPool::EMPTY, &probe, 2),
            vec![Payment::life(2)]
        );
        assert!(Payment::options(&ManaPool::EMPTY, &probe, 1).is_empty());
        assert!(Payment::options(&ManaPool::of(Color::B, 3), &probe, 1).is_empty());
    }

    #[test]
    fn two_phyrexian_pips_can_be_split() {
        let cost = ManaCost::phyrexian(Color::U, 2);
        let mut options = Payment::options(&ManaPool::of(Color::U, 1), &cost, 10);
        options.sort();
        let mut expected = vec![
            Payment {
                mana: ManaPool::of(Color::U, 1),
                life: 2,
                ..Payment::FREE
            },
            Payment::life(4),
        ];
        expected.sort();
        assert_eq!(options, expected);
    }

    #[test]
    fn plain_costs_never_offer_life() {
        let options = Payment::options(
            &ManaPool::of(Color::U, 2),
            &ManaCost::colored(Color::U, 2),
            20,
        );
        assert_eq!(options, vec![Payment::mana(ManaPool::of(Color::U, 2))]);
    }

    #[test]
    fn display() {
        assert_eq!(
            ManaPool::of(Color::U, 2).with(Color::B, 1).to_string(),
            "UUB"
        );
        assert_eq!(ManaPool::EMPTY.to_string(), "0");
        assert_eq!(
            ManaCost::colored(Color::U, 2).with_generic(1).to_string(),
            "{1}{U}{U}"
        );
        assert_eq!(ManaCost::FREE.to_string(), "{0}");
        assert_eq!(ManaCost::phyrexian(Color::U, 1).to_string(), "{U/P}");
        assert_eq!(Payment::FREE.to_string(), "nothing");
        assert_eq!(Payment::life(2).to_string(), "2 life");
        assert_eq!(
            Payment {
                mana: ManaPool::of(Color::U, 1),
                life: 2,
                ..Payment::FREE
            }
            .to_string(),
            "U and 2 life"
        );
        let pitch = Payment {
            alt: Some(AltCost::Pitch(Card::Brainstorm)),
            ..Payment::life(1)
        };
        assert!(!pitch.is_free());
        assert_eq!(pitch.to_string(), "1 life and exiling Brainstorm from hand");
        assert_eq!(
            Payment {
                alt: Some(AltCost::ReturnIsland(Card::Island)),
                ..Payment::FREE
            }
            .to_string(),
            "returning Island to hand"
        );
    }

    #[test]
    fn hybrid_pips_are_paid_with_either_color() {
        let cost = ManaCost::FREE.with_generic(1).with_hybrid(1);
        assert_eq!(cost.to_string(), "{1}{R/G}");
        assert_eq!(cost.mana_value(), 2);
        // R + U, G + U, or R + G either way round.
        let pool = ManaPool::of(Color::R, 1)
            .with(Color::G, 1)
            .with(Color::U, 1);
        let mut payments = pool.payments_for(&cost);
        payments.sort();
        let mut expected = vec![
            ManaPool::of(Color::R, 1).with(Color::U, 1),
            ManaPool::of(Color::G, 1).with(Color::U, 1),
            ManaPool::of(Color::R, 1).with(Color::G, 1),
        ];
        expected.sort();
        assert_eq!(payments, expected);
        assert!(ManaPool::of(Color::U, 2).payments_for(&cost).is_empty());
        assert_eq!(
            ManaPool::of(Color::G, 2).payments_for(&cost),
            vec![ManaPool::of(Color::G, 2)]
        );
    }
}
