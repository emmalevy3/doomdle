//! Thin CLI. With no arguments it verifies a built-in sample pile. Given a
//! puzzle file it searches for a winning pile, and given a file plus card
//! names it verifies that pile:
//!
//! ```text
//! cargo run
//! cargo run -- puzzles/cavern-wizard.json
//! cargo run -- puzzles/cavern-wizard.json "Swamp"
//! ```

use std::error::Error;
use std::{env, fs, process};

use doomdle::{
    Card, Permanent, PlayerState, Puzzle, PuzzleFile, State, Verdict, solve_puzzle, verify,
};

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(path) = args.first() else {
        return verify_sample();
    };
    let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let file = PuzzleFile::parse(&text)?;
    if let Some(title) = &file.title {
        println!("{title}\n");
    }
    let puzzle = file.puzzle();
    if args.len() > 1 {
        let pile = args[1..]
            .iter()
            .map(|name| name.parse::<Card>())
            .collect::<Result<Vec<Card>, _>>()?;
        report(verify(puzzle, &pile)?);
        return Ok(());
    }
    match solve_puzzle(puzzle.clone())? {
        Some((pile, strategy)) => {
            let names: Vec<&str> = pile.iter().map(|c| c.name()).collect();
            println!("Winning pile (top first): {}\n", names.join(", "));
            println!("{}\n", State::with_pile(puzzle, &pile)?);
            println!("{strategy}");
        }
        None => println!("No pile wins against every response."),
    }
    Ok(())
}

fn report(verdict: Verdict) {
    match verdict {
        Verdict::Wins { start, strategy } => {
            println!("{start}\n");
            println!("This pile wins:\n{strategy}");
        }
        Verdict::Fails => println!("This pile does not win against every response."),
    }
}

fn verify_sample() -> Result<(), Box<dyn Error>> {
    let puzzle = Puzzle {
        you: PlayerState {
            life: 20,
            hand: vec![Card::LionsEyeDiamond, Card::Brainstorm],
            battlefield: vec![Permanent::untapped(Card::Island)],
            library: vec![
                Card::AncestralRecall,
                Card::ThassasOracle,
                Card::LotusPetal,
                Card::MoxSapphire,
                Card::BlackLotus,
            ],
            ..PlayerState::default()
        },
        opponent: PlayerState::default(),
    };
    let pile = [
        Card::AncestralRecall,
        Card::ThassasOracle,
        Card::LotusPetal,
        Card::MoxSapphire,
        Card::BlackLotus,
    ];
    report(verify(puzzle, &pile)?);
    Ok(())
}
