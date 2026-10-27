//! Money as a type.
//!
//! [`Cents`] is a count of cents with checked arithmetic only. It has no `+`, `-` or `*`, so every
//! overflow is handled where it can happen, and a quantity or an exchange rate (scaled `i64`s, see
//! `ledger::QUANTITY_SCALE` and `ledger::RATE_SCALE`) cannot be added to an amount by mistake.
//! In JSON and in SQLite it is the bare integer, so the screens and the schema don't change.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, sqlx::Type)]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct Cents(pub i64);

impl Cents {
    pub const ZERO: Cents = Cents(0);

    pub const fn get(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, other: Cents) -> Option<Cents> {
        self.0.checked_add(other.0).map(Cents)
    }

    pub fn checked_sub(self, other: Cents) -> Option<Cents> {
        self.0.checked_sub(other.0).map(Cents)
    }

    /// The total of `amounts`, or `None` if it overflows.
    pub fn checked_sum(amounts: impl IntoIterator<Item = Cents>) -> Option<Cents> {
        amounts.into_iter().try_fold(Cents::ZERO, Cents::checked_add)
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub const fn is_positive(self) -> bool {
        self.0 > 0
    }

    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }
}

impl From<i64> for Cents {
    fn from(cents: i64) -> Self {
        Cents(cents)
    }
}

/// `$1,234.56` / `-$1,234.56`.
impl fmt::Display for Cents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&crate::ledger::format_cents(self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::Cents;

    #[test]
    fn arithmetic_is_checked() {
        assert_eq!(Cents(1).checked_add(Cents(2)), Some(Cents(3)));
        assert_eq!(Cents(i64::MAX).checked_add(Cents(1)), None);
        assert_eq!(Cents(i64::MIN).checked_sub(Cents(1)), None);
        assert_eq!(Cents::checked_sum([Cents(5), Cents(-2), Cents(10)]), Some(Cents(13)));
        assert_eq!(Cents::checked_sum([Cents(i64::MAX), Cents(1)]), None);
        assert_eq!(Cents::checked_sum([]), Some(Cents::ZERO));
    }

    #[test]
    fn is_a_bare_integer_in_json_and_displays_as_money() {
        assert_eq!(serde_json::to_string(&Cents(1050)).unwrap(), "1050");
        assert_eq!(serde_json::from_str::<Cents>("-99").unwrap(), Cents(-99));
        assert_eq!(Cents(123_456).to_string(), "$1,234.56");
        assert_eq!(Cents(-99).to_string(), "-$0.99");
    }
}
