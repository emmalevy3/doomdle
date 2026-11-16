//! Every file in `web/puzzles/` must parse, round-trip, and have a solution the
//! solver confirms. The dated ones (`YYYY-MM-DD.json`, the daily schedule) must
//! also pass `schedule::check_dated`: id = date, a difficulty, no repeats, no
//! missing days.

use std::fs;

use doomdle::schedule::check_dated;
use doomdle::{DEFAULT_BUDGET, Date, PuzzleFile, Verdict, verify, verify_within};

#[test]
fn every_puzzle_file_parses_and_its_solution_wins() {
    let mut checked = 0;
    let mut dated = Vec::new();
    for entry in fs::read_dir("web/puzzles").expect("puzzles directory") {
        let path = entry.expect("directory entry").path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let name = path.display().to_string();
        let text = fs::read_to_string(&path).expect("readable file");
        let file = PuzzleFile::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let again = PuzzleFile::parse(&file.to_json().expect("serializes")).expect("round trip");
        assert_eq!(again, file, "{name} does not round-trip");
        let solution = file
            .solution
            .clone()
            .unwrap_or_else(|| panic!("{name} has no solution"));
        let verdict = verify(file.puzzle(), &solution).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            matches!(verdict, Verdict::Wins { .. }),
            "{name}: the solution does not win"
        );
        // The web build searches under a budget: every shipped puzzle must
        // finish within it.
        let budgeted = verify_within(file.puzzle(), &solution, DEFAULT_BUDGET)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            matches!(budgeted, Verdict::Wins { .. }),
            "{name}: the solution does not win within the default budget"
        );
        checked += 1;
        if let Some(date) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<Date>().ok())
        {
            dated.push((date, file));
        }
    }
    assert!(checked >= 4, "expected the sample puzzles, found {checked}");
    let problems = check_dated(&dated);
    assert!(problems.is_empty(), "the daily schedule: {problems:#?}");
}
