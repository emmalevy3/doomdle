//! Thin CLI: verifies a sample pile and prints the winning line.

use doomdle::{Card, Permanent, PlayerState, Puzzle, Verdict, verify};

fn main() {
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

    match verify(puzzle, &pile) {
        Ok(Verdict::Wins { start, strategy }) => {
            println!("{start}\n");
            println!("This pile wins:\n{strategy}");
        }
        Ok(Verdict::Fails) => println!("This pile does not win against every response."),
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
    }
}
