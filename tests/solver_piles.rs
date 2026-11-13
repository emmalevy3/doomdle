//! Solver tests in pairs: a pile that must be found to win, and a near-miss
//! that must fail.

use doomdle::{
    Action, Card, Outcome, Permanent, PlayerState, Puzzle, State, Verdict, solve, verify,
};

fn puzzle(hand: Vec<Card>, battlefield: Vec<Permanent>, candidates: Vec<Card>) -> Puzzle {
    Puzzle {
        you: PlayerState {
            life: 20,
            hand,
            battlefield,
            library: candidates,
            ..PlayerState::default()
        },
        opponent: PlayerState::default(),
    }
}

fn assert_wins(puzzle: Puzzle, pile: &[Card]) -> Vec<doomdle::Step> {
    match verify(puzzle, pile).expect("legal pile") {
        Verdict::Wins { start, strategy } => {
            let steps = strategy.line(&start).expect("no opponent branching yet");
            assert_eq!(
                steps.last().expect("at least one step").state.outcome,
                Some(Outcome::Win)
            );
            steps
        }
        Verdict::Fails => panic!("expected the pile {pile:?} to win"),
    }
}

fn assert_fails(puzzle: Puzzle, pile: &[Card]) {
    assert_eq!(verify(puzzle, pile).expect("legal pile"), Verdict::Fails);
}

const LED_PILE: [Card; 5] = [
    Card::AncestralRecall,
    Card::ThassasOracle,
    Card::LotusPetal,
    Card::MoxSapphire,
    Card::BlackLotus,
];

fn led_puzzle(hand: Vec<Card>, candidates: Vec<Card>) -> Puzzle {
    puzzle(hand, vec![Permanent::untapped(Card::Island)], candidates)
}

/// Pair 1: Brainstorm with LED in response, put Oracle back, Ancestral draws
/// it again with the library down to one card.
#[test]
fn led_brainstorm_ancestral_wins() {
    let steps = assert_wins(
        led_puzzle(
            vec![Card::LionsEyeDiamond, Card::Brainstorm],
            LED_PILE.to_vec(),
        ),
        &LED_PILE,
    );
    let actions: Vec<&Action> = steps.iter().map(|s| &s.action).collect();
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::ActivateLed { .. }))
    );
    assert!(actions.iter().any(|a| matches!(
        a,
        Action::CastSpell {
            card: Card::AncestralRecall,
            ..
        }
    )));
    // LED has to be cracked with Brainstorm on the stack: check it comes after
    // the Brainstorm cast and before Brainstorm resolves (its put-back choice).
    let cast_at = actions
        .iter()
        .position(|a| {
            matches!(
                a,
                Action::CastSpell {
                    card: Card::Brainstorm,
                    ..
                }
            )
        })
        .unwrap();
    let led_at = actions
        .iter()
        .position(|a| matches!(a, Action::ActivateLed { .. }))
        .unwrap();
    let put_back_at = actions
        .iter()
        .position(|a| matches!(a, Action::Choose(_)))
        .unwrap();
    assert!(cast_at < led_at && led_at < put_back_at);
}

#[test]
fn without_led_the_mana_is_short() {
    assert_fails(
        led_puzzle(vec![Card::Brainstorm], LED_PILE.to_vec()),
        &LED_PILE,
    );
}

#[test]
fn oracle_at_the_bottom_is_the_wrong_order() {
    let pile = [
        Card::AncestralRecall,
        Card::LotusPetal,
        Card::MoxSapphire,
        Card::BlackLotus,
        Card::ThassasOracle,
    ];
    assert_fails(
        led_puzzle(vec![Card::LionsEyeDiamond, Card::Brainstorm], pile.to_vec()),
        &pile,
    );
}

#[test]
fn without_ancestral_too_many_cards_remain() {
    // Brainstorm nets -1, so four cards stay in the library against devotion 2.
    let pile = [
        Card::ThassasOracle,
        Card::LotusPetal,
        Card::MoxSapphire,
        Card::BlackLotus,
        Card::Island,
    ];
    assert_fails(
        led_puzzle(vec![Card::LionsEyeDiamond, Card::Brainstorm], pile.to_vec()),
        &pile,
    );
}

/// Pair 2: Oracle in hand with the mana already there; only the pile size
/// matters.
#[test]
fn oracle_in_hand_wins_when_devotion_covers_the_pile() {
    let battlefield = vec![
        Permanent::untapped(Card::Island),
        Permanent::untapped(Card::Island),
    ];
    // Fewer than five candidates: the whole set is the pile. Empty is fine.
    assert_wins(
        puzzle(vec![Card::ThassasOracle], battlefield.clone(), vec![]),
        &[],
    );
    // Devotion 2 covers a library of exactly 2.
    assert_wins(
        puzzle(
            vec![Card::ThassasOracle],
            battlefield,
            vec![Card::Swamp, Card::Swamp],
        ),
        &[Card::Swamp, Card::Swamp],
    );
}

#[test]
fn oracle_with_three_cards_left_falls_one_short() {
    let battlefield = vec![
        Permanent::untapped(Card::Island),
        Permanent::untapped(Card::Island),
    ];
    let pile = [Card::Swamp, Card::Swamp, Card::Swamp];
    assert_fails(
        puzzle(vec![Card::ThassasOracle], battlefield, pile.to_vec()),
        &pile,
    );
}

#[test]
fn oracle_with_one_island_cannot_be_cast() {
    assert_fails(
        puzzle(
            vec![Card::ThassasOracle],
            vec![Permanent::untapped(Card::Island)],
            vec![],
        ),
        &[],
    );
}

/// Searching from before the pile choice finds a pile on its own.
#[test]
fn solver_finds_a_winning_pile_from_scratch() {
    let start = State::new(led_puzzle(
        vec![Card::LionsEyeDiamond, Card::Brainstorm],
        LED_PILE.to_vec(),
    ));
    let strategy = solve(&start).expect("some order of these five cards wins");
    let steps = strategy.line(&start).unwrap();
    assert!(matches!(steps[0].action, Action::ChoosePile(_)));
    assert_eq!(steps.last().unwrap().state.outcome, Some(Outcome::Win));
}

#[test]
fn no_pile_wins_without_a_win_condition() {
    let start = State::new(led_puzzle(
        vec![Card::LionsEyeDiamond, Card::Brainstorm],
        vec![
            Card::AncestralRecall,
            Card::LotusPetal,
            Card::MoxSapphire,
            Card::BlackLotus,
            Card::Island,
        ],
    ));
    assert_eq!(solve(&start), None);
}
