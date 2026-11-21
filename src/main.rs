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
//! cargo run --release -- schedule --weeks 3 --dry-run
//! cargo run --release -- schedule --from 2026-11-16 --weeks 4 \
//!     --start-with medium,medium,medium --exclude gen-2026-11,gen-2026-96
//! ```
//!
//! `rate` tries every pile and reports how good the puzzle is as a puzzle.
//! `survey` deals random puzzles and tabulates the ratings of the ones that
//! clear the agreed bar; `generate` does the same with optional extra limits.
//! Both write the puzzles they keep to `puzzles/generated/` (gitignored).
//! Options: `--count N` (0: run until stopped), `--seed S`, `--budget B` (per pile), and for
//! `generate` `--max-ideas K`, `--max-sets K`, `--max-share P` (percent),
//! `--min-decoys D`, `--order-matters`, `--difficulty easy|medium|hard`,
//! `--start N` (resume a seed's deals at attempt N).

use std::collections::HashSet;
use std::error::Error;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::{env, fs, process};

use doomdle::{
    Attempt, Card, Criteria, Date, Difficulty, GenerateOptions, Permanent, PlayerState, PoolEntry,
    Puzzle, PuzzleFile, RateOptions, Rating, State, Verdict, WEEK, attempt, fingerprint, rate,
    solve_puzzle, verify,
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
    if first == "schedule" {
        return schedule(&args[1..]);
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
        "--difficulty",
        "--start",
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
            max_share: flag::<f64>(args, "--max-share")?
                .map(|p| p / 100.0)
                .or(Criteria::default().max_share),
            min_decoys: flag(args, "--min-decoys")?,
            order_must_matter: args.iter().any(|a| a == "--order-matters"),
            ..Criteria::default()
        };
        options.difficulty = flag(args, "--difficulty")?;
    }

    fs::create_dir_all(OUT_DIR)?;
    // Every puzzle already generated or on the site, so no run repeats one.
    let mut known = std::collections::HashSet::new();
    for dir in [OUT_DIR, "web/puzzles"] {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json")
                && let Ok(text) = fs::read_to_string(&path)
                && let Ok(file) = PuzzleFile::parse(&text)
            {
                known.insert(fingerprint(&file));
            }
        }
    }
    let goal = if count == 0 {
        "until stopped".to_string()
    } else {
        count.to_string()
    };
    println!(
        "Seed {seed}; keeping {goal}; {} puzzles already known; writing to {OUT_DIR}/\n",
        known.len()
    );
    if survey {
        println!(
            "{:<18} {:<6} {:>5} {:>6} {:>5} {:>5} {:>8} {:>6} {:>5}",
            "puzzle", "tier", "cands", "wins", "sets", "ideas", "share", "decoys", "order"
        );
    }
    let started = Instant::now();
    let (mut screened, mut rejected, mut repeats, mut kept_count) = (0, 0, 0, 0);
    let mut tiers = [0usize; 3]; // easy, medium, hard
    let mut kept: Vec<Rating> = Vec::new(); // only for the survey's summary
    // --start resumes a seed's sequence of deals at a given attempt number.
    let mut n: u64 = flag(args, "--start")?.unwrap_or(0);
    while count == 0 || kept_count < count {
        match attempt(seed, n, &options)? {
            Attempt::Screened => screened += 1,
            Attempt::Rejected(_) => rejected += 1,
            Attempt::Kept(file, _) if !known.insert(fingerprint(&file)) => repeats += 1,
            Attempt::Kept(file, rating) => {
                kept_count += 1;
                match rating.difficulty() {
                    Some(Difficulty::Easy) => tiers[0] += 1,
                    Some(Difficulty::Medium) => tiers[1] += 1,
                    Some(Difficulty::Hard) => tiers[2] += 1,
                    // Only ever set by hand in a file, never by a rating.
                    Some(Difficulty::Expert) | None => {}
                }
                let id = file.id.clone().unwrap_or_else(|| format!("gen-{seed}-{n}"));
                let path = Path::new(OUT_DIR).join(format!("{id}.json"));
                fs::write(&path, file.to_json()?)?;
                if survey {
                    println!(
                        "{:<18} {:<6} {:>5} {:>6} {:>5} {:>5} {:>7.2}% {:>6} {:>5}",
                        id,
                        rating
                            .difficulty()
                            .map_or("-".to_string(), |d| d.to_string()),
                        rating.candidates,
                        rating.winning_piles(),
                        rating.winning_sets.len(),
                        rating.ideas,
                        rating.winning_share() * 100.0,
                        rating.decoys.len(),
                        if rating.order_matters() { "yes" } else { "no" }
                    );
                    kept.push(*rating);
                } else {
                    println!(
                        "[{:.0?}] #{kept_count} {} -> {} (so far {} easy, {} medium, {} hard)\n{rating}",
                        started.elapsed(),
                        id,
                        path.display(),
                        tiers[0],
                        tiers[1],
                        tiers[2]
                    );
                }
            }
        }
        n += 1;
    }
    println!(
        "\nDealt {n}: {screened} had no winning pile, {rejected} fell short, {repeats} repeated a known puzzle, {kept_count} kept, in {:.1?}.",
        started.elapsed()
    );
    if survey {
        summarize(&kept);
    }
    Ok(())
}

const SITE_DIR: &str = "web/puzzles";

/// Reads every puzzle file in `dir` as (file stem, file), sorted by name.
fn read_puzzles(dir: &str) -> Result<Vec<(String, PuzzleFile)>, Box<dyn Error>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let text = fs::read_to_string(&path)?;
        let file = PuzzleFile::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        out.push((stem.to_string(), file));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// `schedule [--from DATE] [--weeks N | --days N] [--dry-run] [--report]`:
/// assigns generated puzzles to the next dates following the weekly mix,
/// re-rating each before it is placed, and writes `web/puzzles/<date>.json`.
fn schedule(args: &[String]) -> Result<(), Box<dyn Error>> {
    use doomdle::schedule::{coverage, pick};

    let with_value = ["--from", "--weeks", "--days", "--exclude", "--start-with"];
    let known = [
        "--from",
        "--weeks",
        "--days",
        "--exclude",
        "--start-with",
        "--dry-run",
        "--report",
    ];
    for (i, arg) in args.iter().enumerate() {
        let is_value = i > 0 && with_value.contains(&args[i - 1].as_str());
        if !is_value && !known.contains(&arg.as_str()) {
            return Err(format!("unknown option {arg:?}").into());
        }
    }
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let report_only = args.iter().any(|a| a == "--report");

    // What is already on the site.
    let mut scheduled: Vec<(Date, PuzzleFile)> = Vec::new();
    for (stem, file) in read_puzzles(SITE_DIR)? {
        if let Ok(date) = stem.parse::<Date>() {
            scheduled.push((date, file));
        }
    }
    let used: HashSet<String> = scheduled.iter().map(|(_, f)| fingerprint(f)).collect();
    let first = scheduled.iter().map(|(d, _)| *d).min();
    let last = scheduled.iter().map(|(d, _)| *d).max();

    // The pool: generated puzzles not yet on the site, each once.
    let mut files: Vec<PuzzleFile> = Vec::new();
    let mut pool: Vec<PoolEntry> = Vec::new();
    let mut seen = HashSet::new();
    // --exclude a,b: generated files (by name) never to schedule.
    let excluded: Vec<String> = flag::<String>(args, "--exclude")?
        .map(|list| list.split(',').map(|n| n.trim().to_string()).collect())
        .unwrap_or_default();
    for (stem, file) in read_puzzles(OUT_DIR)? {
        if excluded.contains(&stem) {
            continue;
        }
        let print = fingerprint(&file);
        if used.contains(&print) || !seen.insert(print) {
            continue;
        }
        pool.push(PoolEntry::new(&stem, file.difficulty));
        files.push(file);
    }

    match (first, last) {
        (Some(first), Some(last)) => println!(
            "On the site: {} dated puzzles, {first} to {last}.",
            scheduled.len()
        ),
        _ => println!("On the site: no dated puzzles yet."),
    }
    let print_coverage = |pool: &[PoolEntry]| {
        let unrated = pool.iter().filter(|e| !e.used && e.tier.is_none()).count();
        println!("Left in {OUT_DIR}/ (weeks at 2 easy, 3 medium, 2 hard a week):");
        for (tier, have, weeks) in coverage(pool, &WEEK) {
            println!("  {tier:<6} {have:>4}  ({weeks} weeks)");
        }
        if unrated > 0 {
            println!("  {unrated} not yet rated (rated when scheduling)");
        }
    };
    if report_only {
        print_coverage(&pool);
        return Ok(());
    }

    let days: usize = match (
        flag::<usize>(args, "--weeks")?,
        flag::<usize>(args, "--days")?,
    ) {
        (Some(_), Some(_)) => return Err("give --weeks or --days, not both".into()),
        (Some(weeks), None) => weeks * 7,
        (None, Some(days)) => days,
        (None, None) => 21,
    };
    let today = Date::from_days(i64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() / 86_400,
    )?);
    let start = match flag::<Date>(args, "--from")? {
        Some(date) => date,
        None => last.map_or(today, Date::next),
    };
    let number_from = first.map_or(start, |f| f.min(start));
    let options = RateOptions::default();
    let criteria = Criteria::default();

    // Rate the pool's unlabelled puzzles first, so every tier is known.
    for (entry, file) in pool.iter_mut().zip(&files) {
        if entry.tier.is_none() {
            let rating = rate(&file.puzzle(), &options)?;
            if rating.check(&criteria).is_empty() {
                entry.tier = rating.difficulty();
            } else {
                entry.used = true; // not good enough to schedule
            }
        }
    }

    println!(
        "\nScheduling {days} days from {start}{}:",
        if dry_run {
            " (dry run: nothing is written)"
        } else {
            ""
        }
    );
    let mut date = start;
    let mut placed = 0;
    // --start-with medium,medium: the tiers of the first days scheduled,
    // before the weekly pattern takes over.
    let start_with: Vec<Difficulty> = match flag::<String>(args, "--start-with")? {
        Some(list) => list
            .split(',')
            .map(|t| t.trim().parse::<Difficulty>())
            .collect::<Result<_, _>>()?,
        None => Vec::new(),
    };
    'days: for _ in 0..days {
        let path = Path::new(SITE_DIR).join(format!("{date}.json"));
        if path.exists() {
            println!("  {date} {}  already has a puzzle", date.weekday_name());
            date = date.next();
            continue;
        }
        let wanted = start_with
            .get(placed)
            .copied()
            .unwrap_or(WEEK[date.weekday()]);
        // Re-rate the pick at today's cutoffs: a puzzle that moved tier goes
        // back in the pool under its new tier, one that fails is dropped.
        let (entry, tier, rating) = loop {
            let Some(i) = pick(&pool, wanted) else {
                println!(
                    "  {date} {}  the pool is empty: stopping",
                    date.weekday_name()
                );
                break 'days;
            };
            let rating = rate(&files[i].puzzle(), &options)?;
            let now = rating.difficulty();
            if !rating.check(&criteria).is_empty() {
                pool[i].used = true;
                continue;
            }
            match now {
                Some(tier) if Some(tier) == pool[i].tier => break (i, tier, rating),
                Some(_) => pool[i].tier = now,
                None => pool[i].used = true,
            }
        };
        pool[entry].used = true;
        let mut file = files[entry].clone();
        file.id = Some(date.to_string());
        file.title = Some(format!("Puzzle #{}", number_from.days_until(date) + 1));
        file.difficulty = Some(tier);
        if file.solution.is_none() {
            file.solution = rating
                .winning_sets
                .first()
                .and_then(|s| s.orderings.first())
                .cloned();
        }
        let note = if tier == wanted {
            String::new()
        } else {
            format!("  (substituted for {wanted})")
        };
        println!(
            "  {date} {}  {tier:<6} {:>5.2}%  {}{note}",
            date.weekday_name(),
            rating.winning_share() * 100.0,
            pool[entry].name,
        );
        if !dry_run {
            fs::write(&path, file.to_json()?)?;
        }
        placed += 1;
        date = date.next();
    }
    println!("\n{placed} scheduled.");
    print_coverage(&pool);
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
    let tier = |d: Difficulty| kept.iter().filter(|r| r.difficulty() == Some(d)).count();
    println!(
        "  tiers: {} easy, {} medium, {} hard",
        tier(Difficulty::Easy),
        tier(Difficulty::Medium),
        tier(Difficulty::Hard)
    );
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
