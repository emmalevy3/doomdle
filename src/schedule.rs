//! The daily schedule: calendar dates (no crate), the weekly mix of
//! difficulties, and assigning puzzles from a pool to dates.

use std::fmt;
use std::str::FromStr;

use std::collections::HashMap;

use crate::generate::fingerprint;
use crate::puzzle_file::PuzzleFile;
use crate::quality::Difficulty;

/// A calendar date (proleptic Gregorian).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl Date {
    /// Days since 1970-01-01 (negative before), by Howard Hinnant's
    /// days-from-civil algorithm.
    pub fn to_days(self) -> i64 {
        let (m, d) = (i64::from(self.month), i64::from(self.day));
        let y = i64::from(self.year) - i64::from(m <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The date `days` after 1970-01-01.
    pub fn from_days(days: i64) -> Date {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        Date {
            // In range for any date this program meets.
            year: year as i32,
            month: month as u8,
            day: day as u8,
        }
    }

    /// The following day.
    pub fn next(self) -> Date {
        Date::from_days(self.to_days() + 1)
    }

    /// Days from `self` to `later` (negative if `later` is earlier).
    pub fn days_until(self, later: Date) -> i64 {
        later.to_days() - self.to_days()
    }

    /// 0 for Monday through 6 for Sunday.
    pub fn weekday(self) -> usize {
        // 1970-01-01 was a Thursday (index 3).
        (self.to_days() + 3).rem_euclid(7) as usize
    }

    /// The weekday's short name.
    pub fn weekday_name(self) -> &'static str {
        ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][self.weekday()]
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// A date written `YYYY-MM-DD`; anything else (including 2026-02-30) is an error.
impl FromStr for Date {
    type Err = String;

    fn from_str(s: &str) -> Result<Date, String> {
        let bad = || format!("not a date (YYYY-MM-DD): {s:?}");
        let parts: Vec<&str> = s.split('-').collect();
        let [y, m, d] = parts.as_slice() else {
            return Err(bad());
        };
        if y.len() != 4 || m.len() != 2 || d.len() != 2 {
            return Err(bad());
        }
        let date = Date {
            year: y.parse().map_err(|_| bad())?,
            month: m.parse().map_err(|_| bad())?,
            day: d.parse().map_err(|_| bad())?,
        };
        // A real date survives the round trip through a day count.
        if !(1..=12).contains(&date.month) || Date::from_days(date.to_days()) != date {
            return Err(bad());
        }
        Ok(date)
    }
}

/// The weekly mix, Monday first: 2 Easy, 3 Medium, 2 Hard, with the hard days
/// at the weekend.
pub const WEEK: [Difficulty; 7] = [
    Difficulty::Easy,
    Difficulty::Medium,
    Difficulty::Medium,
    Difficulty::Easy,
    Difficulty::Medium,
    Difficulty::Hard,
    Difficulty::Hard,
];

/// The tiers to try, in order, when `wanted` has run out: the nearest first.
pub fn fallbacks(wanted: Difficulty) -> [Difficulty; 3] {
    use Difficulty::*;
    match wanted {
        Easy => [Easy, Medium, Hard],
        Medium => [Medium, Easy, Hard],
        // Never asked for (no week has an Expert day), but treated as Hard.
        Hard | Expert => [Hard, Medium, Easy],
    }
}

/// A puzzle available for scheduling: its tier (`None` until rated) and
/// whether it is already used or ruled out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolEntry {
    pub name: String,
    pub tier: Option<Difficulty>,
    pub used: bool,
}

impl PoolEntry {
    pub fn new(name: &str, tier: Option<Difficulty>) -> PoolEntry {
        PoolEntry {
            name: name.to_string(),
            tier,
            used: false,
        }
    }
}

/// The first unused pool entry for a day that wants `wanted`, falling back to
/// the nearest tier. Entries are taken in pool order.
pub fn pick(pool: &[PoolEntry], wanted: Difficulty) -> Option<usize> {
    fallbacks(wanted)
        .into_iter()
        .find_map(|tier| pool.iter().position(|e| !e.used && e.tier == Some(tier)))
}

/// One scheduled day.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assignment {
    pub date: Date,
    /// Index into the pool.
    pub entry: usize,
    pub wanted: Difficulty,
    pub tier: Difficulty,
}

impl Assignment {
    /// True if the day's tier ran out and another tier stood in.
    pub fn substituted(&self) -> bool {
        self.wanted != self.tier
    }
}

/// Assigns pool puzzles to `days` dates from `start` following `week`,
/// marking them used. Stops early when the pool runs dry.
pub fn plan(
    start: Date,
    days: usize,
    pool: &mut [PoolEntry],
    week: &[Difficulty; 7],
) -> Vec<Assignment> {
    let mut out = Vec::new();
    let mut date = start;
    for _ in 0..days {
        let wanted = week[date.weekday()];
        let Some(entry) = pick(pool, wanted) else {
            break;
        };
        pool[entry].used = true;
        let tier = pool[entry].tier.unwrap_or(wanted);
        out.push(Assignment {
            date,
            entry,
            wanted,
            tier,
        });
        date = date.next();
    }
    out
}

/// How many unused puzzles of each tier (easy, medium, hard) the pool holds,
/// and how many whole weeks of `week` each covers.
pub fn coverage(pool: &[PoolEntry], week: &[Difficulty; 7]) -> [(Difficulty, usize, usize); 3] {
    use Difficulty::*;
    [Easy, Medium, Hard].map(|tier| {
        let have = pool
            .iter()
            .filter(|e| !e.used && e.tier == Some(tier))
            .count();
        let per_week = week.iter().filter(|t| **t == tier).count();
        (tier, have, have.checked_div(per_week).unwrap_or(0))
    })
}

/// The problems with a set of dated puzzle files (`YYYY-MM-DD.json`): an
/// `id` that isn't the file's date, a missing `difficulty`, the same puzzle on
/// two dates, or a day with no puzzle between the first and last date. Empty
/// if all is well.
pub fn check_dated(files: &[(Date, PuzzleFile)]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut sorted: Vec<&(Date, PuzzleFile)> = files.iter().collect();
    sorted.sort_by_key(|(date, _)| *date);
    let mut seen: HashMap<String, Date> = HashMap::new();
    for (date, file) in &sorted {
        if file.id.as_deref() != Some(date.to_string().as_str()) {
            problems.push(format!("{date}: id is {:?}, not the date", file.id));
        }
        if file.difficulty.is_none() {
            problems.push(format!("{date}: no difficulty"));
        }
        if let Some(earlier) = seen.insert(fingerprint(file), *date) {
            problems.push(format!("{date}: the same puzzle as {earlier}"));
        }
    }
    for pair in sorted.windows(2) {
        let (a, b) = (pair[0].0, pair[1].0);
        if a.days_until(b) > 1 {
            problems.push(format!(
                "no puzzle from {} to {}",
                a.next(),
                Date::from_days(b.to_days() - 1)
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::{Rng, deal};
    use Difficulty::*;

    fn date(s: &str) -> Date {
        s.parse().expect("a date")
    }

    #[test]
    fn dates_parse_print_and_step() {
        assert_eq!(date("2026-11-16").to_string(), "2026-11-16");
        assert_eq!(date("2026-11-30").next(), date("2026-12-01"));
        assert_eq!(date("2026-12-31").next(), date("2027-01-01"));
        assert_eq!(date("2028-02-28").next(), date("2028-02-29"));
        assert_eq!(date("2027-02-28").next(), date("2027-03-01"));
        assert_eq!(date("1970-01-01").to_days(), 0);
        assert_eq!(date("2026-11-16").days_until(date("2026-12-16")), 30);
        for bad in ["2026-02-30", "2026-13-01", "26-11-16", "2026-11", "today"] {
            assert!(bad.parse::<Date>().is_err(), "{bad}");
        }
    }

    #[test]
    fn weekdays_are_right() {
        assert_eq!(date("2026-11-16").weekday_name(), "Mon");
        assert_eq!(date("2026-11-22").weekday_name(), "Sun");
        assert_eq!(date("2000-01-01").weekday_name(), "Sat");
        assert_eq!(date("1970-01-01").weekday_name(), "Thu");
    }

    #[test]
    fn the_week_is_two_easy_three_medium_two_hard() {
        let count = |t| WEEK.iter().filter(|w| **w == t).count();
        assert_eq!((count(Easy), count(Medium), count(Hard)), (2, 3, 2));
    }

    fn pool(tiers: &[Difficulty]) -> Vec<PoolEntry> {
        tiers
            .iter()
            .enumerate()
            .map(|(i, t)| PoolEntry::new(&format!("p{i}"), Some(*t)))
            .collect()
    }

    #[test]
    fn plan_follows_the_week_from_a_midweek_start() {
        let mut entries = pool(&[Hard, Hard, Medium, Medium, Medium, Easy, Easy]);
        // Thursday 2026-11-19: Easy, Medium, Hard, Hard, Easy, Medium, Medium.
        let days = plan(date("2026-11-19"), 7, &mut entries, &WEEK);
        let tiers: Vec<Difficulty> = days.iter().map(|a| a.tier).collect();
        assert_eq!(tiers, vec![Easy, Medium, Hard, Hard, Easy, Medium, Medium]);
        assert!(days.iter().all(|a| !a.substituted()));
        assert_eq!(days[6].date, date("2026-11-25"));
        let mut used: Vec<usize> = days.iter().map(|a| a.entry).collect();
        used.sort();
        used.dedup();
        assert_eq!(used.len(), 7, "no puzzle twice");
        assert!(entries.iter().all(|e| e.used));
    }

    #[test]
    fn a_missing_tier_is_substituted_and_marked() {
        let mut entries = pool(&[Hard, Medium]);
        // Monday wants Easy: none left, Medium stands in; Tuesday then gets Hard.
        let days = plan(date("2026-11-16"), 3, &mut entries, &WEEK);
        assert_eq!(days.len(), 2, "stops when the pool is empty");
        assert_eq!((days[0].wanted, days[0].tier), (Easy, Medium));
        assert!(days[0].substituted());
        assert_eq!((days[1].wanted, days[1].tier), (Medium, Hard));
    }

    #[test]
    fn plan_is_deterministic_and_skips_used_and_unrated() {
        let mut a = pool(&[Medium, Easy, Medium, Hard]);
        a[1].used = true;
        a.push(PoolEntry::new("unrated", None));
        let mut b = a.clone();
        let first = plan(date("2026-11-16"), 3, &mut a, &WEEK);
        let second = plan(date("2026-11-16"), 3, &mut b, &WEEK);
        assert_eq!(first, second);
        assert!(first.iter().all(|d| d.entry != 1 && d.entry != 4));
    }

    fn dated(day: &str, seed: u64) -> (Date, PuzzleFile) {
        let mut file = deal(&mut Rng::new(seed));
        file.id = Some(day.to_string());
        file.difficulty = Some(Hard);
        (date(day), file)
    }

    #[test]
    fn dated_files_in_order_pass() {
        let files = vec![
            dated("2026-11-17", 2),
            dated("2026-11-16", 1),
            dated("2026-11-18", 3),
        ];
        assert_eq!(check_dated(&files), Vec::<String>::new());
    }

    #[test]
    fn dated_file_problems_are_each_named() {
        let mut wrong_id = dated("2026-11-17", 2);
        wrong_id.1.id = Some("gen-1-2".to_string());
        let mut no_tier = dated("2026-11-18", 3);
        no_tier.1.difficulty = None;
        let mut repeat = dated("2026-11-21", 1);
        repeat.1.title = Some("Another title".to_string());
        let files = vec![dated("2026-11-16", 1), wrong_id, no_tier, repeat];
        let problems = check_dated(&files);
        assert_eq!(problems.len(), 4, "{problems:?}");
        for needle in [
            "2026-11-17: id is",
            "2026-11-18: no difficulty",
            "2026-11-21: the same puzzle as 2026-11-16",
            "no puzzle from 2026-11-19 to 2026-11-20",
        ] {
            assert!(
                problems.iter().any(|p| p.contains(needle)),
                "{needle}: {problems:?}"
            );
        }
    }

    #[test]
    fn coverage_counts_weeks_per_tier() {
        let mut entries = pool(&[Easy, Easy, Easy, Medium, Medium, Medium, Medium, Hard]);
        entries[0].used = true;
        assert_eq!(
            coverage(&entries, &WEEK),
            [(Easy, 2, 1), (Medium, 4, 1), (Hard, 1, 0)]
        );
    }
}
