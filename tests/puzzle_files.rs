//! Every file in `puzzles/` must parse, round-trip, and have a solution the
//! solver confirms.

use std::fs;

use doomdle::{PuzzleFile, Verdict, verify};

#[test]
fn every_puzzle_file_parses_and_its_solution_wins() {
    let mut checked = 0;
    for entry in fs::read_dir("puzzles").expect("puzzles directory") {
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
        checked += 1;
    }
    assert!(checked >= 4, "expected the sample puzzles, found {checked}");
}
