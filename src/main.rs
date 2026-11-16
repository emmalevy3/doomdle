//! Thin CLI. With no arguments it verifies a built-in sample pile. Given a
//! puzzle file it searches for a winning pile, and given a file plus card
//! names it verifies that pile:
//!
//! ```text
//! cargo run
//! cargo run -- web/puzzles/cavern-wizard.json
//! cargo run -- web/puzzles/cavern-wizard.json "Swamp"
//! cargo run --release -- rate web/puzzles/led-brainstorm.json
//! cargo run --release -- survey --count 30
//! cargo run --release -- generate --count 5 --max-ideas 2 --max-share 1
//! ```
//!
//! `rate` tries every pile and reports how good the puzzle is as a puzzle.
//! `survey` deals random puzzles and tabulates the ratings of the ones that
//! clear the agreed bar; `generate` does the same with optional extra limits.
//! Both write the puzzles they keep to `puzzles/generated/` (gitignored).
//! Options: `--count N`, `--seed S`, `--budget B` (per pile), and for
//! `generate` `--max-ideas K`, `--max-sets K`, `--max-share P` (percent),
//! `--min-decoys D`, `--order-matters`.

use std::error::Error;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::{env, fs, process};

use doomdle::{
    Attempt, Card, Criteria, GenerateOptions, Permanent, PlayerState, Puzzle, PuzzleFile,
    RateOptions, Rating, State, Verdict, attempt, rate, solve_puzzle, verify,
};

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(first) = args.first() else {
        return verify_sample();
    };
    if first == "rate" {
        let path = args.get(1).ok_or("usage: rate <puzzle.json>")?;
        return rate_file(path);
    }
    if first == "survey" || first == "generate" {
        return generate(first == "survey", &args[1..]);
    }
    let path = first;
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

fn rate_file(path: &str) -> Result<(), Box<dyn Error>> {
    let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let file = PuzzleFile::parse(&text)?;
    if let Some(title) = &file.title {
        println!("{title}\n");
    }
    let started = Instant::now();
    let rating = rate(&file.puzzle(), &RateOptions::default())?;
    print!("{rating}");
    let reasons = rating.check(&Criteria::default());
    if reasons.is_empty() {
        println!("Meets the bar for a generated puzzle.");
    } else {
        println!(
            "Short of the bar for a generated puzzle: {}.",
            reasons.join("; ")
        );
    }
    println!("Rated in {:.1?}.", started.elapsed());
    Ok(())
}

/// The value after `--name`, parsed, if the flag is present.
fn flag<T: std::str::FromStr>(args: &[String], name: &str) -> Result<Option<T>, String> {
    match args.iter().position(|a| a == name) {
        None => Ok(None),
        Some(i) => {
            let value = args.get(i + 1).ok_or(format!("{name} needs a value"))?;
            value
                .parse()
                .map(Some)
                .map_err(|_| format!("{name}: can't read {value:?}"))
        }
    }
}

const OUT_DIR: &str = "puzzles/generated";

/// `survey` (only the agreed bar, then a table) or `generate` (with any
/// extra limits given).
fn generate(survey: bool, args: &[String]) -> Result<(), Box<dyn Error>> {
    let known = [
        "--count",
        "--seed",
        "--budget",
        "--max-ideas",
        "--max-sets",
        "--max-share",
        "--min-decoys",
        "--order-matters",
    ];
    for (i, arg) in args.iter().enumerate() {
        let is_value =
            i > 0 && known.contains(&args[i - 1].as_str()) && args[i - 1] != "--order-matters";
        if !is_value && !known.contains(&arg.as_str()) {
            return Err(format!("unknown option {arg:?}").into());
        }
    }
    let count: usize = flag(args, "--count")?.unwrap_or(if survey { 30 } else { 5 });
    let seed: u64 = match flag(args, "--seed")? {
        Some(seed) => seed,
        None => SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
    };
    let mut options = GenerateOptions::default();
    if let Some(budget) = flag(args, "--budget")? {
        options.rate.per_pile_budget = budget;
    }
    if !survey {
        options.criteria = Criteria {
            max_ideas: flag(args, "--max-ideas")?,
            max_sets: flag(args, "--max-sets")?,
            max_share: flag::<f64>(args, "--max-share")?.map(|p| p / 100.0),
            min_decoys: flag(args, "--min-decoys")?,
            order_must_matter: args.iter().any(|a| a == "--order-matters"),
            ..Criteria::default()
        };
    }

    fs::create_dir_all(OUT_DIR)?;
    println!("Seed {seed}; keeping {count}; writing to {OUT_DIR}/\n");
    if survey {
        println!(
            "{:<18} {:>5} {:>6} {:>5} {:>5} {:>8} {:>6} {:>5}",
            "puzzle", "cands", "wins", "sets", "ideas", "share", "decoys", "order"
        );
    }
    let started = Instant::now();
    let (mut screened, mut rejected) = (0, 0);
    let mut kept: Vec<Rating> = Vec::new();
    let mut n = 0;
    while kept.len() < count {
        match attempt(seed, n, &options)? {
            Attempt::Screened => screened += 1,
            Attempt::Rejected(_) => rejected += 1,
            Attempt::Kept(file, rating) => {
                let id = file.id.clone().unwrap_or_else(|| format!("gen-{seed}-{n}"));
                let path = Path::new(OUT_DIR).join(format!("{id}.json"));
                fs::write(&path, file.to_json()?)?;
                if survey {
                    println!(
                        "{:<18} {:>5} {:>6} {:>5} {:>5} {:>7.2}% {:>6} {:>5}",
                        id,
                        rating.candidates,
                        rating.winning_piles(),
                        rating.winning_sets.len(),
                        rating.ideas,
                        rating.winning_share() * 100.0,
                        rating.decoys.len(),
                        if rating.order_matters() { "yes" } else { "no" }
                    );
                } else {
                    println!("{} -> {}\n{rating}", id, path.display());
                }
                kept.push(*rating);
            }
        }
        n += 1;
    }
    println!(
        "\nDealt {n}: {screened} had no winning pile, {rejected} fell short, {} kept, in {:.1?}.",
        kept.len(),
        started.elapsed()
    );
    if survey {
        summarize(&kept);
    }
    Ok(())
}

/// The spread of each measure over the kept puzzles: lowest, median, highest.
fn summarize(kept: &[Rating]) {
    let spread = |mut values: Vec<f64>| -> String {
        values.sort_by(f64::total_cmp);
        match (values.first(), values.get(values.len() / 2), values.last()) {
            (Some(lo), Some(mid), Some(hi)) => format!("{lo:.2} / {mid:.2} / {hi:.2}"),
            _ => "-".to_string(),
        }
    };
    println!("\nLowest / median / highest:");
    println!(
        "  winning card sets: {}",
        spread(kept.iter().map(|r| r.winning_sets.len() as f64).collect())
    );
    println!(
        "  distinct answers:  {}",
        spread(kept.iter().map(|r| r.ideas as f64).collect())
    );
    println!(
        "  % of piles win:    {}",
        spread(kept.iter().map(|r| r.winning_share() * 100.0).collect())
    );
    println!(
        "  decoys:            {}",
        spread(kept.iter().map(|r| r.decoys.len() as f64).collect())
    );
    let ordered = kept.iter().filter(|r| r.order_matters()).count();
    println!("  order matters in {ordered} of {}", kept.len());
}

fn report(verdict: Verdict) {
    match verdict {
        Verdict::Wins { start, strategy } => {
            println!("{start}\n");
            println!("This pile wins:\n{strategy}");
        }
        Verdict::Fails => println!("This pile does not win against every response."),
        // The CLI searches without a budget, so this never happens here.
        Verdict::GaveUp => println!("The search gave up before finishing."),
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
