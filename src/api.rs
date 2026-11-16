//! The string-in, string-out API the WebAssembly build exposes (and anything
//! else that would rather talk JSON than Rust types). Every function returns
//! a JSON object with an `ok` field and never panics: a malformed file, an
//! unknown card name or an illegal pile come back as `{"ok": false,
//! "error": "..."}`.
//!
//! Searches run under [`DEFAULT_BUDGET`] so a browser tab can't be frozen by
//! a pathological puzzle; a search that runs out reports `"gave_up": true`.

use serde::Serialize;

use crate::cards::Card;
use crate::feedback::{closest_line, opponent_culprits};
use crate::puzzle_file::PuzzleFile;
use crate::solver::{PileSearch, Verdict, solve_puzzle_within, verify_within};
use crate::state::{Puzzle, State};

/// The most states one search may visit before it gives up. The corpus test
/// keeps every shipped puzzle well under this.
pub const DEFAULT_BUDGET: usize = 2_000_000;

/// `verify_pile`: `puzzle_json` is a puzzle file, `pile_json` a JSON array of
/// card names, top first.
///
/// `{"ok":true,"wins":true,"line":"1. ..."}`,
/// `{"ok":true,"wins":false,"gave_up":false,"feedback":{...}}` or
/// `{"ok":false,"error":"..."}`. The feedback on a failed pile:
/// `{"wins_without_their_hand":true,"stopped_by":["Counterspell"],
/// "closest":{"cards_left":1,"life":6,"ended":"you drew from an empty
/// library","line":"1. ..."}}`; `wins_without_their_hand` and `stopped_by`
/// only appear when the opponent holds cards, and each part is left out if
/// its search runs out of budget.
pub fn verify_json(puzzle_json: &str, pile_json: &str) -> String {
    let report = || -> Result<VerifyReport, String> {
        let file = PuzzleFile::parse(puzzle_json).map_err(|e| e.to_string())?;
        let pile: Vec<Card> = serde_json::from_str(pile_json).map_err(|e| format!("pile: {e}"))?;
        let verdict =
            verify_within(file.puzzle(), &pile, DEFAULT_BUDGET).map_err(|e| e.to_string())?;
        Ok(match verdict {
            Verdict::Wins { strategy, .. } => VerifyReport {
                ok: true,
                wins: true,
                gave_up: false,
                line: Some(strategy.to_string()),
                feedback: None,
            },
            Verdict::Fails => VerifyReport {
                ok: true,
                wins: false,
                gave_up: false,
                line: None,
                feedback: Some(feedback(&file.puzzle(), &pile)?),
            },
            Verdict::GaveUp => VerifyReport {
                ok: true,
                wins: false,
                gave_up: true,
                line: None,
                feedback: None,
            },
        })
    };
    to_json(report())
}

/// `solve_puzzle`: searches every legal pile of the puzzle file.
///
/// `{"ok":true,"pile":["..."],"gave_up":false,"line":"1. ..."}`,
/// `{"ok":true,"pile":null,"gave_up":false}` (no pile wins),
/// `{"ok":true,"pile":null,"gave_up":true}` or `{"ok":false,"error":"..."}`.
pub fn solve_json(puzzle_json: &str) -> String {
    let report = || -> Result<SolveReport, String> {
        let file = PuzzleFile::parse(puzzle_json).map_err(|e| e.to_string())?;
        let search =
            solve_puzzle_within(file.puzzle(), DEFAULT_BUDGET).map_err(|e| e.to_string())?;
        Ok(match search {
            PileSearch::Won { pile, strategy } => SolveReport {
                ok: true,
                pile: Some(pile),
                gave_up: false,
                line: Some(strategy.to_string()),
            },
            PileSearch::Lost => SolveReport {
                ok: true,
                pile: None,
                gave_up: false,
                line: None,
            },
            PileSearch::GaveUp => SolveReport {
                ok: true,
                pile: None,
                gave_up: true,
                line: None,
            },
        })
    };
    to_json(report())
}

/// Why a pile failed, as far as the two feedback searches can tell.
fn feedback(puzzle: &Puzzle, pile: &[Card]) -> Result<Feedback, String> {
    let culprits = opponent_culprits(puzzle, pile, DEFAULT_BUDGET).map_err(|e| e.to_string())?;
    let start = State::with_pile(puzzle.clone(), pile).map_err(|e| e.to_string())?;
    let closest = closest_line(&start, DEFAULT_BUDGET).map(|line| Closest {
        cards_left: line.cards_left(),
        life: line.end.you.life,
        ended: line
            .end
            .outcome
            .map(|o| o.to_string())
            .unwrap_or_else(|| "the line stops".to_string()),
        line: line.to_string(),
    });
    Ok(Feedback {
        wins_without_their_hand: culprits.as_ref().map(|c| c.wins_without_their_hand),
        stopped_by: culprits.map(|c| c.stopped_by),
        closest,
    })
}

/// `card_names`: every card in the pool by printed name, as a JSON array.
pub fn card_names_json() -> String {
    to_json(Ok::<_, String>(Card::ALL.to_vec()))
}

#[derive(Serialize)]
struct VerifyReport {
    ok: bool,
    wins: bool,
    gave_up: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    feedback: Option<Feedback>,
}

#[derive(Serialize)]
struct Feedback {
    #[serde(skip_serializing_if = "Option::is_none")]
    wins_without_their_hand: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stopped_by: Option<Vec<Card>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    closest: Option<Closest>,
}

#[derive(Serialize)]
struct Closest {
    cards_left: usize,
    life: i16,
    ended: String,
    line: String,
}

#[derive(Serialize)]
struct SolveReport {
    ok: bool,
    pile: Option<Vec<Card>>,
    gave_up: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<String>,
}

#[derive(Serialize)]
struct ErrorReport {
    ok: bool,
    error: String,
}

/// Serializes a report, or the error it failed with. Serializing these plain
/// structs can't really fail, but the fallback keeps the promise that the
/// output is always a JSON object.
fn to_json<T: Serialize>(report: Result<T, String>) -> String {
    let serialized = match report {
        Ok(report) => serde_json::to_string(&report),
        Err(error) => serde_json::to_string(&ErrorReport { ok: false, error }),
    };
    serialized.unwrap_or_else(|e| format!("{{\"ok\":false,\"error\":{:?}}}", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const CAVERN: &str = r#"{
      "you": {
        "hand": ["Thassa's Oracle"],
        "battlefield": ["Island", "Island", {"card": "Cavern of Souls", "type": "Wizard"}],
        "library": ["Swamp"]
      },
      "opponent": { "hand": ["Counterspell"], "battlefield": ["Island", "Island"] }
    }"#;

    const LED: &str = r#"{
      "you": {
        "hand": ["Lion's Eye Diamond", "Brainstorm"],
        "battlefield": ["Island"],
        "library": ["Ancestral Recall", "Thassa's Oracle", "Lotus Petal", "Mox Sapphire", "Black Lotus"]
      }
    }"#;

    fn parsed(json: &str) -> Value {
        serde_json::from_str(json).expect("the API always returns JSON")
    }

    #[test]
    fn card_names_lists_the_whole_pool() {
        let names: Vec<String> = serde_json::from_str(&card_names_json()).unwrap();
        assert_eq!(names.len(), Card::ALL.len());
        assert!(names.iter().any(|n| n == "Tamiyo, Seasoned Scholar"));
    }

    #[test]
    fn a_winning_pile_reports_the_line() {
        let out = parsed(&verify_json(CAVERN, r#"["Swamp"]"#));
        assert_eq!(out["ok"], true);
        assert_eq!(out["wins"], true);
        assert_eq!(out["gave_up"], false);
        let line = out["line"].as_str().expect("a line");
        assert!(line.contains("Cast Thassa's Oracle"));
        assert!(line.trim_end().ends_with("You win."));
    }

    #[test]
    fn a_losing_pile_reports_no_win() {
        let wrong_order = r#"["Ancestral Recall", "Lotus Petal", "Mox Sapphire", "Black Lotus", "Thassa's Oracle"]"#;
        let out = parsed(&verify_json(LED, wrong_order));
        assert_eq!(out["ok"], true);
        assert_eq!(out["wins"], false);
        assert_eq!(out["gave_up"], false);
        assert!(out.get("line").is_none());
        // Feedback: no opponent hand, so only the closest line.
        let feedback = &out["feedback"];
        assert!(feedback.get("stopped_by").is_none());
        let closest = &feedback["closest"];
        assert!(closest["cards_left"].as_u64().is_some());
        assert!(closest["ended"].as_str().is_some());
        assert!(closest["line"].as_str().expect("a line").contains("Then "));
    }

    #[test]
    fn a_pile_stopped_by_counterspell_says_so() {
        // The Cavern puzzle with the Cavern swapped for an Island.
        let plain = CAVERN.replace(
            r#"{"card": "Cavern of Souls", "type": "Wizard"}"#,
            r#""Island""#,
        );
        let out = parsed(&verify_json(&plain, r#"["Swamp"]"#));
        assert_eq!(out["wins"], false);
        let feedback = &out["feedback"];
        assert_eq!(feedback["wins_without_their_hand"], true);
        assert_eq!(feedback["stopped_by"], serde_json::json!(["Counterspell"]));
        assert!(feedback["closest"]["line"].as_str().is_some());
    }

    #[test]
    fn errors_are_json_objects_with_ok_false() {
        for (puzzle, pile, expect) in [
            ("{", r#"["Swamp"]"#, "puzzle file"),
            (r#"{"you": {"hand": ["Thassa's Oracel"]}}"#, "[]", "Oracel"),
            (CAVERN, r#"["Swmap"]"#, "Swmap"),
            (CAVERN, r#"["Island"]"#, "illegal action"),
            (CAVERN, "not json", "pile"),
        ] {
            let out = parsed(&verify_json(puzzle, pile));
            assert_eq!(out["ok"], false, "{puzzle} / {pile}");
            let error = out["error"].as_str().expect("an error message");
            assert!(error.contains(expect), "{error}");
        }
    }

    #[test]
    fn solve_finds_a_pile_or_says_none_does() {
        let out = parsed(&solve_json(CAVERN));
        assert_eq!(out["ok"], true);
        assert_eq!(out["pile"], serde_json::json!(["Swamp"]));
        assert_eq!(out["gave_up"], false);
        assert!(out["line"].as_str().is_some());
        let hopeless = r#"{"you": {"hand": [], "library": ["Swamp"]}}"#;
        let out = parsed(&solve_json(hopeless));
        assert_eq!(out["ok"], true);
        assert_eq!(out["pile"], Value::Null);
        assert_eq!(out["gave_up"], false);
        assert_eq!(parsed(&solve_json("{"))["ok"], false);
    }
}
