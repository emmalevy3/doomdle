//! Mana as simple per-color counts (plus colorless), per CLAUDE.md.

use std::fmt;

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

/// Mana available to spend: five colors plus colorless. Also used to describe a
/// payment (how much of each kind is spent on a spell).
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

    /// Every distinct way to pay `cost` out of this pool. Colored pips must be
    /// paid exactly; generic mana can come from anything left over. Empty if
    /// the pool can't cover the cost.
    pub fn payments_for(&self, cost: &ManaCost) -> Vec<ManaPool> {
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

/// A mana cost: colored pips plus a generic amount.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ManaCost {
    pips: [u8; 5],
    pub generic: u8,
}

impl ManaCost {
    pub const FREE: ManaCost = ManaCost {
        pips: [0; 5],
        generic: 0,
    };

    /// `n` pips of one color and nothing else.
    pub const fn colored(color: Color, n: u8) -> ManaCost {
        let mut pips = [0; 5];
        pips[color.index()] = n;
        ManaCost { pips, generic: 0 }
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

    pub fn pip(&self, color: Color) -> u8 {
        self.pips[color.index()]
    }

    pub fn mana_value(&self) -> u8 {
        self.pips.iter().sum::<u8>() + self.generic
    }

    fn pips_as_pool(&self) -> ManaPool {
        let mut pool = ManaPool::EMPTY;
        for color in Color::ALL {
            pool.add_mana(color, self.pip(color));
        }
        pool
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
        Ok(())
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
    }
}
