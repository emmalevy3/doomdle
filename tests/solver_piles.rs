//! Solver tests in pairs: a pile that must be found to win, and a near-miss
//! that must fail.

use doomdle::{
    Action, Card, Outcome, Permanent, Player, PlayerState, Puzzle, State, Target, Verdict, Zone,
    solve, verify,
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

fn has_action(steps: &[doomdle::Step], pred: impl Fn(&Action) -> bool) -> bool {
    steps.iter().any(|s| pred(&s.action))
}

fn two_islands() -> Vec<Permanent> {
    vec![
        Permanent::untapped(Card::Island),
        Permanent::untapped(Card::Island),
    ]
}

// ---------------------------------------------------------------- stage 1

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
    // Fewer than five candidates: the whole set is the pile. Empty is fine.
    assert_wins(
        puzzle(vec![Card::ThassasOracle], two_islands(), vec![]),
        &[],
    );
    // Devotion 2 covers a library of exactly 2.
    assert_wins(
        puzzle(
            vec![Card::ThassasOracle],
            two_islands(),
            vec![Card::Swamp, Card::Swamp],
        ),
        &[Card::Swamp, Card::Swamp],
    );
}

#[test]
fn oracle_with_three_cards_left_falls_one_short() {
    let pile = [Card::Swamp, Card::Swamp, Card::Swamp];
    assert_fails(
        puzzle(vec![Card::ThassasOracle], two_islands(), pile.to_vec()),
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

// ---------------------------------------------------------------- stage 2

const THREE_SWAMPS: [Card; 3] = [Card::Swamp, Card::Swamp, Card::Swamp];

/// Pair 3: Gitaxian Probe for 2 life shrinks the pile to Oracle range.
#[test]
fn probe_for_life_wins() {
    let steps = assert_wins(
        puzzle(
            vec![Card::GitaxianProbe, Card::ThassasOracle],
            two_islands(),
            THREE_SWAMPS.to_vec(),
        ),
        &THREE_SWAMPS,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::GitaxianProbe,
            payment,
            ..
        } if payment.life == 2
    )));
}

#[test]
fn probe_for_life_at_two_life_kills_you() {
    // 3 life -> 2 after Doomsday. Paying 2 life hits 0; paying U leaves one
    // Island for a UU Oracle.
    let mut p = puzzle(
        vec![Card::GitaxianProbe, Card::ThassasOracle],
        two_islands(),
        THREE_SWAMPS.to_vec(),
    );
    p.you.life = 3;
    assert_fails(p.clone(), &THREE_SWAMPS);
    // 2 life -> 1: life can't be paid at all.
    p.you.life = 2;
    assert_fails(p, &THREE_SWAMPS);
}

/// Pair 4: a fetchland pulls a dual out of the pile, shrinking it and adding
/// the second blue source.
#[test]
fn fetching_a_dual_out_of_the_pile_wins() {
    let pile = [Card::Swamp, Card::UndergroundSea, Card::Swamp];
    let steps = assert_wins(
        puzzle(
            vec![Card::ThassasOracle],
            vec![
                Permanent::untapped(Card::PollutedDelta),
                Permanent::untapped(Card::Island),
            ],
            pile.to_vec(),
        ),
        &pile,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::ActivateFetch {
            source: Card::PollutedDelta
        }
    )));
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::Choose(doomdle::Choice::SearchLand(Some(Card::UndergroundSea)))
    )));
}

#[test]
fn fetching_leaves_too_many_cards_or_is_impossible() {
    let pile = [Card::Swamp, Card::UndergroundSea, Card::Swamp, Card::Swamp];
    assert_fails(
        puzzle(
            vec![Card::ThassasOracle],
            vec![
                Permanent::untapped(Card::PollutedDelta),
                Permanent::untapped(Card::Island),
            ],
            pile.to_vec(),
        ),
        &pile,
    );
    let pile = [Card::Swamp, Card::UndergroundSea, Card::Swamp];
    assert_fails(
        puzzle(
            vec![Card::ThassasOracle],
            vec![
                Permanent::tapped(Card::PollutedDelta),
                Permanent::untapped(Card::Island),
            ],
            pile.to_vec(),
        ),
        &pile,
    );
}

/// Pair 5: a surveil land played as the land drop bins the top of the pile.
#[test]
fn surveil_land_shrinks_the_pile() {
    let steps = assert_wins(
        puzzle(
            vec![Card::ThassasOracle, Card::UndercitySewers],
            two_islands(),
            THREE_SWAMPS.to_vec(),
        ),
        &THREE_SWAMPS,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::PlayLand {
            card: Card::UndercitySewers,
            ..
        }
    )));
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::Choose(doomdle::Choice::Surveil { to_graveyard: true })
    )));
}

#[test]
fn surveil_land_needs_the_land_drop() {
    let mut p = puzzle(
        vec![Card::ThassasOracle, Card::UndercitySewers],
        two_islands(),
        THREE_SWAMPS.to_vec(),
    );
    p.you.land_drop_available = false;
    assert_fails(p, &THREE_SWAMPS);
}

/// Pair 6: Doctor Doom draws the last card and wins at 0 life.
#[test]
fn doom_draws_the_last_card_and_wins_at_zero_life() {
    let mut p = puzzle(
        vec![],
        vec![Permanent::untapped(Card::DoctorDoom)],
        vec![Card::Swamp],
    );
    p.you.life = 2;
    let steps = assert_wins(p, &[Card::Swamp]);
    assert!(has_action(&steps, |a| matches!(a, Action::ActivateDoom)));
    assert_eq!(steps.last().unwrap().state.you.life, 0);
}

#[test]
fn doom_fails_with_two_cards_or_without_being_ready() {
    let two = [Card::Swamp, Card::Swamp];
    assert_fails(
        puzzle(
            vec![],
            vec![Permanent::untapped(Card::DoctorDoom)],
            two.to_vec(),
        ),
        &two,
    );
    assert_fails(
        puzzle(
            vec![],
            vec![Permanent::tapped(Card::DoctorDoom)],
            vec![Card::Swamp],
        ),
        &[Card::Swamp],
    );
    assert_fails(
        puzzle(
            vec![],
            vec![Permanent::entering(Card::DoctorDoom, false)],
            vec![Card::Swamp],
        ),
        &[Card::Swamp],
    );
}

/// Pair 7: Jace +1 on yourself mills two and draws from the empty library.
#[test]
fn jace_plus_one_wins_on_a_two_card_pile() {
    let two = [Card::Swamp, Card::Swamp];
    let steps = assert_wins(
        puzzle(
            vec![],
            vec![Permanent::untapped(Card::JaceWielderOfMysteries)],
            two.to_vec(),
        ),
        &two,
    );
    assert!(has_action(&steps, |a| matches!(a, Action::Loyalty { .. })));
}

#[test]
fn jace_plus_one_on_three_cards_draws_the_last_one_and_stops() {
    assert_fails(
        puzzle(
            vec![],
            vec![Permanent::untapped(Card::JaceWielderOfMysteries)],
            THREE_SWAMPS.to_vec(),
        ),
        &THREE_SWAMPS,
    );
}

#[test]
fn jace_cast_this_turn_can_still_activate() {
    let two = [Card::Swamp, Card::Swamp];
    let lands = vec![
        Permanent::untapped(Card::Island),
        Permanent::untapped(Card::Island),
        Permanent::untapped(Card::Island),
        Permanent::untapped(Card::Swamp),
    ];
    let steps = assert_wins(
        puzzle(
            vec![Card::JaceWielderOfMysteries],
            lands.clone(),
            two.to_vec(),
        ),
        &two,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::JaceWielderOfMysteries,
            ..
        }
    )));
    // One blue source short: {1}{U}{U}{U} needs three blue.
    let mut short = lands;
    short[2] = Permanent::untapped(Card::Swamp);
    assert_fails(
        puzzle(vec![Card::JaceWielderOfMysteries], short, two.to_vec()),
        &two,
    );
}

/// Pair 8: Laboratory Maniac turns Brainstorm's third draw into the win.
#[test]
fn lab_man_wins_on_brainstorms_third_draw() {
    let two = [Card::Swamp, Card::Swamp];
    assert_wins(
        puzzle(
            vec![Card::Brainstorm],
            vec![
                Permanent::untapped(Card::LaboratoryManiac),
                Permanent::untapped(Card::Island),
            ],
            two.to_vec(),
        ),
        &two,
    );
}

#[test]
fn lab_man_with_three_cards_never_draws_from_empty() {
    assert_fails(
        puzzle(
            vec![Card::Brainstorm],
            vec![
                Permanent::untapped(Card::LaboratoryManiac),
                Permanent::untapped(Card::Island),
            ],
            THREE_SWAMPS.to_vec(),
        ),
        &THREE_SWAMPS,
    );
}

/// Pair 9: Edge of Autumn, both modes.
#[test]
fn edge_of_autumn_cycled_for_a_tapped_land_draws_into_lab_man() {
    let steps = assert_wins(
        puzzle(
            vec![Card::EdgeOfAutumn],
            vec![
                Permanent::untapped(Card::LaboratoryManiac),
                Permanent::tapped(Card::Island),
            ],
            vec![],
        ),
        &[],
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::Cycle {
            card: Card::EdgeOfAutumn,
            sacrifice: Some(Card::Island),
            ..
        }
    )));
}

#[test]
fn edge_of_autumn_cannot_be_cycled_without_a_land() {
    assert_fails(
        puzzle(
            vec![Card::EdgeOfAutumn],
            vec![Permanent::untapped(Card::LaboratoryManiac)],
            vec![],
        ),
        &[],
    );
}

#[test]
fn edge_of_autumn_cast_pulls_a_basic_out_of_the_pile() {
    // Tropical Island (G) + Island (generic) pay for Edge; Island + Mox pay for
    // Oracle. The library drops to two, unordered, and Oracle wins on size.
    let battlefield = vec![
        Permanent::untapped(Card::TropicalIsland),
        Permanent::untapped(Card::Island),
        Permanent::untapped(Card::Island),
        Permanent::untapped(Card::MoxSapphire),
    ];
    let steps = assert_wins(
        puzzle(
            vec![Card::EdgeOfAutumn, Card::ThassasOracle],
            battlefield.clone(),
            THREE_SWAMPS.to_vec(),
        ),
        &THREE_SWAMPS,
    );
    // Either mode gets there; the solver picks one and the line replays.
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::EdgeOfAutumn,
            ..
        } | Action::Cycle {
            card: Card::EdgeOfAutumn,
            ..
        }
    )));
    // Four cards in the pile: neither mode removes enough.
    let four = [Card::Swamp, Card::Swamp, Card::Swamp, Card::Swamp];
    assert_fails(
        puzzle(
            vec![Card::EdgeOfAutumn, Card::ThassasOracle],
            battlefield,
            four.to_vec(),
        ),
        &four,
    );
}

// ---------------------------------------------------------------- stage 3

/// Pair 10: Consider bins Oracle, Unearth brings it back, devotion covers the
/// two cards left.
#[test]
fn unearth_returns_a_surveilled_oracle() {
    let lands = vec![
        Permanent::untapped(Card::UndergroundSea),
        Permanent::untapped(Card::Island),
        Permanent::untapped(Card::Swamp),
    ];
    let pile = [
        Card::ThassasOracle,
        Card::Plains,
        Card::Plains,
        Card::Plains,
    ];
    let steps = assert_wins(
        puzzle(
            vec![Card::Consider, Card::Unearth],
            lands.clone(),
            pile.to_vec(),
        ),
        &pile,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::Unearth,
            target: Some(Target::Graveyard {
                owner: Player::You,
                card: Card::ThassasOracle
            }),
            ..
        }
    )));
    // One card more and three remain after Consider.
    let five = [
        Card::ThassasOracle,
        Card::Plains,
        Card::Plains,
        Card::Plains,
        Card::Plains,
    ];
    assert_fails(
        puzzle(
            vec![Card::Consider, Card::Unearth],
            lands.clone(),
            five.to_vec(),
        ),
        &five,
    );
    // Without Consider, Oracle never reaches the graveyard: no target.
    assert_fails(puzzle(vec![Card::Unearth], lands, pile.to_vec()), &pile);
}

/// Pair 11: Yawgmoth's Will with LED cracked in response, then LED and the
/// discarded Oracle recast from the graveyard.
#[test]
fn yawgmoths_will_recasts_led_and_oracle_from_the_graveyard() {
    let board = vec![
        Permanent::untapped(Card::LionsEyeDiamond),
        Permanent::untapped(Card::Swamp),
        Permanent::untapped(Card::Swamp),
        Permanent::untapped(Card::Swamp),
    ];
    let two = [Card::Island, Card::Island];
    let steps = assert_wins(
        puzzle(
            vec![Card::YawgmothsWill, Card::ThassasOracle],
            board.clone(),
            two.to_vec(),
        ),
        &two,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::ThassasOracle,
            from: Zone::Graveyard,
            ..
        }
    )));
    // Without Will, LED's discard just loses Oracle.
    assert_fails(
        puzzle(vec![Card::ThassasOracle], board.clone(), two.to_vec()),
        &two,
    );
    // Three cards in the pile is one too many for devotion 2.
    let three = [Card::Island, Card::Island, Card::Island];
    assert_fails(
        puzzle(
            vec![Card::YawgmothsWill, Card::ThassasOracle],
            board,
            three.to_vec(),
        ),
        &three,
    );
}

/// Pair 12: Dark Ritual puts an instant next to Doomsday, so Flow State takes
/// both mana rocks and Oracle wins on the two cards left.
#[test]
fn flow_state_two_card_mode_finds_both_rocks() {
    let pile = [
        Card::LotusPetal,
        Card::MoxSapphire,
        Card::Swamp,
        Card::Swamp,
    ];
    let steps = assert_wins(
        puzzle(
            vec![Card::DarkRitual, Card::FlowState, Card::ThassasOracle],
            vec![
                Permanent::untapped(Card::UndergroundSea),
                Permanent::untapped(Card::Island),
            ],
            pile.to_vec(),
        ),
        &pile,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::Choose(doomdle::Choice::FlowState { to_hand, .. }) if to_hand.len() == 2
    )));
    // Same mana without the instant: one-card mode leaves three in the pile.
    assert_fails(
        puzzle(
            vec![Card::FlowState, Card::ThassasOracle],
            vec![
                Permanent::untapped(Card::UndergroundSea),
                Permanent::untapped(Card::Island),
                Permanent::untapped(Card::Swamp),
            ],
            pile.to_vec(),
        ),
        &pile,
    );
}

/// Pair 13: Frantic Search is free with three lands to untap; the untapped
/// Islands then pay for Oracle.
#[test]
fn frantic_search_untaps_the_lands_that_pay_for_oracle() {
    let pile = [Card::Plains, Card::Plains, Card::Plains, Card::Plains];
    let steps = assert_wins(
        puzzle(
            vec![Card::FranticSearch, Card::ThassasOracle],
            vec![
                Permanent::untapped(Card::Island),
                Permanent::untapped(Card::Island),
                Permanent::untapped(Card::UndergroundSea),
            ],
            pile.to_vec(),
        ),
        &pile,
    );
    // Both chained choices are replayed in the line.
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::Choose(doomdle::Choice::Discard(_))
    )));
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::Choose(doomdle::Choice::UntapLands(lands)) if lands.len() == 3
    )));
    // Two lands can't cast Frantic Search at all.
    assert_fails(
        puzzle(
            vec![Card::FranticSearch, Card::ThassasOracle],
            vec![
                Permanent::untapped(Card::Island),
                Permanent::untapped(Card::UndergroundSea),
            ],
            pile.to_vec(),
        ),
        &pile,
    );
}

/// Pair 14: Cabal Ritual's net mana lets Lab Man come down before Frantic
/// Search draws the last card and then one more.
#[test]
fn cabal_ritual_pays_for_lab_man_before_frantic_search() {
    let lands = |swamps: usize| {
        let mut v = vec![
            Permanent::untapped(Card::Island),
            Permanent::untapped(Card::Island),
        ];
        v.extend(std::iter::repeat_n(
            Permanent::untapped(Card::Swamp),
            swamps,
        ));
        v
    };
    let hand = vec![
        Card::CabalRitual,
        Card::LaboratoryManiac,
        Card::FranticSearch,
    ];
    let one = [Card::Plains];
    let steps = assert_wins(puzzle(hand.clone(), lands(3), one.to_vec()), &one);
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::CabalRitual,
            ..
        }
    )));
    // One land fewer: five mana can't pay for Lab Man and Frantic Search.
    assert_fails(puzzle(hand, lands(2), one.to_vec()), &one);
}

// --------------------------------------------------------------- stage 4a

fn opponent(hand: Vec<Card>, lands: usize) -> PlayerState {
    PlayerState {
        hand,
        battlefield: vec![Permanent::untapped(Card::Island); lands],
        ..PlayerState::default()
    }
}

fn puzzle_vs(
    hand: Vec<Card>,
    battlefield: Vec<Permanent>,
    candidates: Vec<Card>,
    opponent: PlayerState,
) -> Puzzle {
    Puzzle {
        opponent,
        ..puzzle(hand, battlefield, candidates)
    }
}

/// Like `assert_wins`, but the opponent may branch: every branch must end in
/// a win. Returns every step of every branch.
fn assert_wins_vs(puzzle: Puzzle, pile: &[Card]) -> Vec<doomdle::Step> {
    match verify(puzzle, pile).expect("legal pile") {
        Verdict::Wins { start, strategy } => {
            let leaves = strategy.outcomes(&start).expect("strategy replays");
            assert!(!leaves.is_empty());
            assert!(leaves.iter().all(|s| s.outcome == Some(Outcome::Win)));
            let mut steps = Vec::new();
            strategy
                .walk(&start, &mut |_, step| steps.push(step.clone()))
                .expect("strategy replays");
            steps
        }
        Verdict::Fails => panic!("expected the pile {pile:?} to win against the opponent"),
    }
}

fn islands(n: usize) -> Vec<Permanent> {
    vec![Permanent::untapped(Card::Island); n]
}

/// Pair 15: Counterspell. Oracle alone loses to it; Pact of Negation answers
/// it; with one Island the opponent can't cast it at all.
#[test]
fn counterspell_is_beaten_by_pact_of_negation() {
    let one = [Card::Swamp];
    let counterspell = || opponent(vec![Card::Counterspell], 2);
    assert_wins(
        puzzle(vec![Card::ThassasOracle], islands(2), one.to_vec()),
        &one,
    );
    assert_fails(
        puzzle_vs(
            vec![Card::ThassasOracle],
            islands(2),
            one.to_vec(),
            counterspell(),
        ),
        &one,
    );
    let steps = assert_wins_vs(
        puzzle_vs(
            vec![Card::ThassasOracle, Card::PactOfNegation],
            islands(2),
            one.to_vec(),
            counterspell(),
        ),
        &one,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::Counterspell,
            ..
        }
    )));
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::PactOfNegation,
            target: Some(Target::Stack {
                card: Card::Counterspell,
                ..
            }),
            ..
        }
    )));
    // One Island: Counterspell is never castable, so the plain pile wins.
    assert_wins_vs(
        puzzle_vs(
            vec![Card::ThassasOracle],
            islands(2),
            one.to_vec(),
            opponent(vec![Card::Counterspell], 1),
        ),
        &one,
    );
}

/// Pair 16: Daze. Spending every mana on Oracle loses to it; a third Island
/// is tapped in response and pays the {1}.
#[test]
fn daze_is_beaten_by_leaving_one_open() {
    let one = [Card::Swamp];
    let daze = || opponent(vec![Card::Daze], 1);
    assert_fails(
        puzzle_vs(vec![Card::ThassasOracle], islands(2), one.to_vec(), daze()),
        &one,
    );
    let steps = assert_wins_vs(
        puzzle_vs(vec![Card::ThassasOracle], islands(3), one.to_vec(), daze()),
        &one,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::Daze,
            payment: doomdle::Payment {
                alt: Some(doomdle::AltCost::ReturnIsland(Card::Island)),
                ..
            },
            ..
        }
    )));
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::Choose(doomdle::Choice::PayTax(_))
    )));
}

/// Pair 17: Force of Will. One Oracle loses to it; a second Oracle gets
/// through; without a blue card to pitch (and no lands) Force is dead.
#[test]
fn force_of_will_is_beaten_by_a_second_oracle() {
    let one = [Card::Swamp];
    let force = || opponent(vec![Card::ForceOfWill, Card::Brainstorm], 0);
    assert_fails(
        puzzle_vs(vec![Card::ThassasOracle], islands(2), one.to_vec(), force()),
        &one,
    );
    let steps = assert_wins_vs(
        puzzle_vs(
            vec![Card::ThassasOracle, Card::ThassasOracle],
            islands(4),
            one.to_vec(),
            force(),
        ),
        &one,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::ForceOfWill,
            payment: doomdle::Payment {
                alt: Some(doomdle::AltCost::Pitch(Card::Brainstorm)),
                life: 1,
                ..
            },
            ..
        }
    )));
    assert_wins_vs(
        puzzle_vs(
            vec![Card::ThassasOracle],
            islands(2),
            one.to_vec(),
            opponent(vec![Card::ForceOfWill, Card::Swamp], 0),
        ),
        &one,
    );
}

/// Pair 18: Stifle. Oracle's trigger is stifled; Laboratory Maniac wins
/// through a replacement effect Stifle can't touch.
#[test]
fn stifle_is_beaten_by_lab_man() {
    let one = [Card::Swamp];
    let stifle = || opponent(vec![Card::Stifle], 1);
    assert_fails(
        puzzle_vs(
            vec![Card::ThassasOracle],
            islands(2),
            one.to_vec(),
            stifle(),
        ),
        &one,
    );
    let steps = assert_wins_vs(
        puzzle_vs(
            vec![Card::LaboratoryManiac, Card::Brainstorm],
            islands(4),
            one.to_vec(),
            stifle(),
        ),
        &one,
    );
    // Nothing of yours is an ability, so Stifle never even gets cast.
    assert!(!has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::Stifle,
            ..
        }
    )));
}

/// Pair 19: Spell Pierce can't touch Oracle, but it hits the Brainstorm that
/// finds it unless {2} is left open.
#[test]
fn spell_pierce_on_the_cantrip_needs_two_open() {
    let pile = [Card::ThassasOracle, Card::Swamp, Card::Plains];
    let pierce = || opponent(vec![Card::SpellPierce], 1);
    assert_wins(
        puzzle(vec![Card::Brainstorm], islands(3), pile.to_vec()),
        &pile,
    );
    assert_fails(
        puzzle_vs(vec![Card::Brainstorm], islands(3), pile.to_vec(), pierce()),
        &pile,
    );
    let steps = assert_wins_vs(
        puzzle_vs(vec![Card::Brainstorm], islands(5), pile.to_vec(), pierce()),
        &pile,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::Choose(doomdle::Choice::PayTax(mana)) if mana.total() == 2
    )));
    // Oracle is never a Spell Pierce target.
    assert!(!has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::SpellPierce,
            target: Some(Target::Stack {
                card: Card::ThassasOracle,
                ..
            }),
            ..
        }
    )));
}

// --------------------------------------------------------------- stage 4b

/// Pair 20: Pyroblast kills Oracle under its trigger. Hydroblast in hand
/// answers the Pyroblast.
#[test]
fn pyroblast_on_oracle_is_beaten_by_hydroblast() {
    let one = [Card::Swamp];
    let pyroblast = || opponent(vec![Card::Pyroblast], 0);
    let red = PlayerState {
        battlefield: vec![Permanent::untapped(Card::Mountain)],
        ..pyroblast()
    };
    assert_fails(
        puzzle_vs(
            vec![Card::ThassasOracle],
            islands(2),
            one.to_vec(),
            red.clone(),
        ),
        &one,
    );
    let steps = assert_wins_vs(
        puzzle_vs(
            vec![Card::ThassasOracle, Card::Hydroblast],
            islands(3),
            one.to_vec(),
            red,
        ),
        &one,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::Pyroblast,
            target: Some(Target::Permanent {
                card: Card::ThassasOracle,
                ..
            }),
            ..
        }
    )));
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::Hydroblast,
            target: Some(Target::Stack {
                card: Card::Pyroblast,
                ..
            }),
            ..
        }
    )));
    // No red source: Pyroblast is dead and plain Oracle wins.
    assert_wins_vs(
        puzzle_vs(
            vec![Card::ThassasOracle],
            islands(2),
            one.to_vec(),
            pyroblast(),
        ),
        &one,
    );
}

/// Pair 21: Snuff Out kills Laboratory Maniac in response to the draw; Oracle
/// as a second win condition gets around it.
#[test]
fn snuff_out_on_lab_man_is_beaten_by_a_second_win_condition() {
    let one = [Card::Swamp];
    let snuff = || PlayerState {
        battlefield: vec![Permanent::untapped(Card::Swamp)],
        ..opponent(vec![Card::SnuffOut], 0)
    };
    assert_fails(
        puzzle_vs(
            vec![Card::LaboratoryManiac, Card::Brainstorm],
            islands(4),
            one.to_vec(),
            snuff(),
        ),
        &one,
    );
    let steps = assert_wins_vs(
        puzzle_vs(
            vec![
                Card::LaboratoryManiac,
                Card::Brainstorm,
                Card::ThassasOracle,
            ],
            islands(6),
            one.to_vec(),
            snuff(),
        ),
        &one,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::SnuffOut,
            payment: doomdle::Payment { life: 4, .. },
            ..
        }
    )));
}

/// Pair 22: Surgical Extraction on Doomsday shuffles the pile. A Consider
/// line that looks at one card of three dies to it; Oracle with devotion for
/// the whole pile doesn't care.
#[test]
fn surgical_on_doomsday_scrambles_the_pile() {
    let surgical = || PlayerState {
        battlefield: vec![Permanent::untapped(Card::Swamp)],
        ..opponent(vec![Card::SurgicalExtraction], 0)
    };
    let pile = [Card::Swamp, Card::Plains, Card::Island];
    let hand = || vec![Card::Consider, Card::ThassasOracle];
    assert_wins(puzzle(hand(), islands(3), pile.to_vec()), &pile);
    assert_fails(
        puzzle_vs(hand(), islands(3), pile.to_vec(), surgical()),
        &pile,
    );
    let two = [Card::Swamp, Card::Plains];
    let steps = assert_wins_vs(
        puzzle_vs(
            vec![Card::ThassasOracle],
            islands(2),
            two.to_vec(),
            surgical(),
        ),
        &two,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::SurgicalExtraction,
            target: Some(Target::Graveyard {
                card: Card::Doomsday,
                ..
            }),
            ..
        }
    )));
}

/// Pair 23: Flusterstorm with storm three taxes Dark Ritual three times. Oracle
/// cast off lands instead can't be targeted.
#[test]
fn flusterstorm_taxes_the_ritual_but_not_the_creature() {
    let one = [Card::Swamp];
    let fluster = || opponent(vec![Card::Flusterstorm], 1);
    // Doomsday was the third spell: storm 3 on the Ritual needs {3} spare.
    let after_two = |hand, battlefield| {
        let mut p = puzzle_vs(hand, battlefield, one.to_vec(), fluster());
        p.you.spells_cast_this_turn = 2;
        p
    };
    assert_fails(
        after_two(
            vec![Card::DarkRitual, Card::LaboratoryManiac, Card::Brainstorm],
            vec![
                Permanent::untapped(Card::Swamp),
                Permanent::untapped(Card::Island),
                Permanent::untapped(Card::Island),
            ],
        ),
        &one,
    );
    // Same hand, but Oracle wins off two Islands with nothing to Flusterstorm.
    let steps = assert_wins_vs(
        after_two(
            vec![Card::DarkRitual, Card::ThassasOracle],
            vec![
                Permanent::untapped(Card::Swamp),
                Permanent::untapped(Card::Island),
                Permanent::untapped(Card::Island),
            ],
        ),
        &one,
    );
    assert!(!has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::Flusterstorm,
            ..
        }
    )));
}

/// Pair 24: Mindbreak Trap is free once you have cast three spells. A long
/// line loses to it; a two-spell line leaves it uncastable.
#[test]
fn mindbreak_trap_punishes_the_long_line() {
    let trap = || opponent(vec![Card::MindbreakTrap], 1);
    // Doomsday, Brainstorm, then Oracle is the third spell: Trap is free.
    let pile = [Card::ThassasOracle, Card::Swamp, Card::Plains];
    assert_fails(
        puzzle_vs(vec![Card::Brainstorm], islands(3), pile.to_vec(), trap()),
        &pile,
    );
    // Oracle straight from hand is only the second spell.
    let one = [Card::Swamp];
    let steps = assert_wins_vs(
        puzzle_vs(vec![Card::ThassasOracle], islands(2), one.to_vec(), trap()),
        &one,
    );
    assert!(!has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::MindbreakTrap,
            ..
        }
    )));
}

// --------------------------------------------------------------- stage 4c

/// Pair 25: Duress strips Counterspell before Oracle; Thoughtseize at 2 life
/// after Doomsday kills you instead.
#[test]
fn duress_clears_the_way_for_oracle() {
    let one = [Card::Swamp];
    let counterspell = || opponent(vec![Card::Counterspell], 2);
    let lands = || {
        vec![
            Permanent::untapped(Card::Swamp),
            Permanent::untapped(Card::Island),
            Permanent::untapped(Card::Island),
        ]
    };
    assert_fails(
        puzzle_vs(
            vec![Card::ThassasOracle],
            lands(),
            one.to_vec(),
            counterspell(),
        ),
        &one,
    );
    let steps = assert_wins_vs(
        puzzle_vs(
            vec![Card::Duress, Card::ThassasOracle],
            lands(),
            one.to_vec(),
            counterspell(),
        ),
        &one,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::Choose(doomdle::Choice::Discard(cards)) if cards == &[Card::Counterspell]
    )));
    // Thoughtseize works the same way with life to spare, and kills you at 4
    // (2 after Doomsday).
    assert_wins_vs(
        puzzle_vs(
            vec![Card::Thoughtseize, Card::ThassasOracle],
            lands(),
            one.to_vec(),
            counterspell(),
        ),
        &one,
    );
    let mut low = puzzle_vs(
        vec![Card::Thoughtseize, Card::ThassasOracle],
        lands(),
        one.to_vec(),
        counterspell(),
    );
    low.you.life = 4;
    assert_fails(low, &one);
}

/// Pair 26: Silence before Oracle against Force of Will: they Force the
/// Silence or can't cast afterwards.
#[test]
fn silence_beats_force_of_will() {
    let one = [Card::Swamp];
    let force = || opponent(vec![Card::ForceOfWill, Card::Brainstorm], 0);
    let lands = || {
        vec![
            Permanent::untapped(Card::Plains),
            Permanent::untapped(Card::Island),
            Permanent::untapped(Card::Island),
        ]
    };
    assert_fails(
        puzzle_vs(vec![Card::ThassasOracle], lands(), one.to_vec(), force()),
        &one,
    );
    let steps = assert_wins_vs(
        puzzle_vs(
            vec![Card::Silence, Card::ThassasOracle],
            lands(),
            one.to_vec(),
            force(),
        ),
        &one,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::Silence,
            ..
        }
    )));
}

/// Pair 27: Veil of Summer gives Laboratory Maniac hexproof from Snuff Out.
#[test]
fn veil_protects_lab_man_from_snuff_out() {
    let one = [Card::Swamp];
    let snuff = || PlayerState {
        battlefield: vec![Permanent::untapped(Card::Swamp)],
        ..opponent(vec![Card::SnuffOut], 0)
    };
    let lands = |forest: bool| {
        let mut v = islands(4);
        if forest {
            v.push(Permanent::untapped(Card::Forest));
        }
        v
    };
    assert_fails(
        puzzle_vs(
            vec![Card::LaboratoryManiac, Card::Brainstorm],
            lands(false),
            one.to_vec(),
            snuff(),
        ),
        &one,
    );
    let steps = assert_wins_vs(
        puzzle_vs(
            vec![Card::LaboratoryManiac, Card::Brainstorm, Card::VeilOfSummer],
            lands(true),
            one.to_vec(),
            snuff(),
        ),
        &one,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::VeilOfSummer,
            ..
        }
    )));
}

/// Pair 28: Teferi resolves and the opponent can't cast anything afterwards,
/// or they counter Teferi and Oracle is clear.
#[test]
fn teferi_locks_out_the_counterspell() {
    let one = [Card::Swamp];
    let counterspell = || opponent(vec![Card::Counterspell], 2);
    let lands = || {
        let mut v = islands(4);
        v.push(Permanent::untapped(Card::Plains));
        v
    };
    assert_fails(
        puzzle_vs(
            vec![Card::ThassasOracle],
            lands(),
            one.to_vec(),
            counterspell(),
        ),
        &one,
    );
    let steps = assert_wins_vs(
        puzzle_vs(
            vec![Card::Teferi, Card::ThassasOracle],
            lands(),
            one.to_vec(),
            counterspell(),
        ),
        &one,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::Teferi,
            ..
        }
    )));
}

/// Pair 29: Defense Grid on your side makes Daze cost {3}, which one Island
/// can't pay.
#[test]
fn defense_grid_turns_off_daze() {
    let one = [Card::Swamp];
    let daze = || opponent(vec![Card::Daze], 1);
    assert_fails(
        puzzle_vs(vec![Card::ThassasOracle], islands(2), one.to_vec(), daze()),
        &one,
    );
    let mut board = islands(2);
    board.push(Permanent::untapped(Card::DefenseGrid));
    let steps = assert_wins_vs(
        puzzle_vs(vec![Card::ThassasOracle], board, one.to_vec(), daze()),
        &one,
    );
    assert!(!has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::Daze,
            ..
        }
    )));
}

/// Pair 30: Grief evoked for Dark Ritual takes Force of Will, then Oracle
/// resolves.
#[test]
fn evoked_grief_takes_the_force() {
    let one = [Card::Swamp];
    let force = || opponent(vec![Card::ForceOfWill, Card::Brainstorm], 0);
    assert_fails(
        puzzle_vs(
            vec![Card::ThassasOracle, Card::DarkRitual],
            islands(2),
            one.to_vec(),
            force(),
        ),
        &one,
    );
    let steps = assert_wins_vs(
        puzzle_vs(
            vec![Card::ThassasOracle, Card::DarkRitual, Card::Grief],
            islands(2),
            one.to_vec(),
            force(),
        ),
        &one,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::Grief,
            payment: doomdle::Payment {
                alt: Some(doomdle::AltCost::Pitch(Card::DarkRitual)),
                ..
            },
            ..
        }
    )));
}

// ---------------------------------------------------------------- stage 5

fn board(cards: &[Card]) -> PlayerState {
    PlayerState {
        battlefield: cards.iter().map(|&c| Permanent::untapped(c)).collect(),
        ..PlayerState::default()
    }
}

/// Pair 31: Narset. Brainstorm into Lab Man draws nothing useful; Oracle with
/// devotion for the pile doesn't draw at all.
#[test]
fn narset_stops_the_draw_lines() {
    let narset = || board(&[Card::Narset]);
    let one = [Card::Swamp];
    assert_wins(
        puzzle(
            vec![Card::LaboratoryManiac, Card::Brainstorm],
            islands(4),
            one.to_vec(),
        ),
        &one,
    );
    assert_fails(
        puzzle_vs(
            vec![Card::LaboratoryManiac, Card::Brainstorm],
            islands(4),
            one.to_vec(),
            narset(),
        ),
        &one,
    );
    let two = [Card::Swamp, Card::Plains];
    assert_wins_vs(
        puzzle_vs(
            vec![Card::ThassasOracle],
            islands(2),
            two.to_vec(),
            narset(),
        ),
        &two,
    );
    // Pyroblast on Narset first brings the draws back.
    let mut red = islands(4);
    red.push(Permanent::untapped(Card::Mountain));
    let steps = assert_wins_vs(
        puzzle_vs(
            vec![Card::LaboratoryManiac, Card::Brainstorm, Card::Pyroblast],
            red,
            one.to_vec(),
            narset(),
        ),
        &one,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::Pyroblast,
            target: Some(Target::Permanent {
                card: Card::Narset,
                ..
            }),
            ..
        }
    )));
}

/// Pair 32: Orcish Bowmasters. Each draw pings; at low life the cantrip line
/// kills you, and a lone Lab Man gets shot down.
#[test]
fn bowmasters_punish_draws() {
    let bowmasters = || board(&[Card::OrcishBowmasters]);
    let pile = [Card::ThassasOracle, Card::Swamp, Card::Plains];
    // Brainstorm draws three: three pings. Life 8 is 4 after Doomsday.
    let mut low = puzzle_vs(
        vec![Card::Brainstorm],
        islands(3),
        pile.to_vec(),
        bowmasters(),
    );
    low.you.life = 8;
    assert_wins(
        {
            let mut p = low.clone();
            p.opponent = PlayerState::default();
            p
        },
        &pile,
    );
    let mut six = low.clone();
    six.you.life = 6;
    assert_fails(six, &pile);
    let steps = assert_wins_vs(low, &pile);
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::Choose(doomdle::Choice::Ping(Target::Player(Player::You)))
    )));
    // Lab Man at 3 life against three cards: drawing them before Lab Man
    // means three pings to the face, drawing them after means two on Lab
    // Man. Either way the empty draw never comes with Lab Man alive.
    let three = [Card::Swamp, Card::Plains, Card::Mountain];
    let hand = || {
        vec![
            Card::LaboratoryManiac,
            Card::Ponder,
            Card::GitaxianProbe,
            Card::GitaxianProbe,
            Card::GitaxianProbe,
        ]
    };
    let mut quiet = puzzle(hand(), islands(7), three.to_vec());
    quiet.you.life = 6;
    assert_wins(quiet, &three);
    let mut shot = puzzle_vs(hand(), islands(7), three.to_vec(), bowmasters());
    shot.you.life = 6;
    assert_fails(shot, &three);
    // Oracle as a backup: one draw to the face, then devotion covers two.
    let mut backup = hand();
    backup.push(Card::ThassasOracle);
    let mut saved = puzzle_vs(backup, islands(9), three.to_vec(), bowmasters());
    saved.you.life = 6;
    assert_wins_vs(saved, &three);
}

/// Pair 33: Leyline of the Void. The Will + LED recast line has nothing to
/// recast; a plain Oracle line doesn't care.
#[test]
fn leyline_turns_off_the_graveyard() {
    let leyline = || board(&[Card::LeylineOfTheVoid]);
    let lands = || {
        vec![
            Permanent::untapped(Card::LionsEyeDiamond),
            Permanent::untapped(Card::Swamp),
            Permanent::untapped(Card::Swamp),
            Permanent::untapped(Card::Swamp),
        ]
    };
    let two = [Card::Island, Card::Island];
    assert_wins(
        puzzle(
            vec![Card::YawgmothsWill, Card::ThassasOracle],
            lands(),
            two.to_vec(),
        ),
        &two,
    );
    assert_fails(
        puzzle_vs(
            vec![Card::YawgmothsWill, Card::ThassasOracle],
            lands(),
            two.to_vec(),
            leyline(),
        ),
        &two,
    );
    assert_wins_vs(
        puzzle_vs(
            vec![Card::ThassasOracle],
            islands(2),
            two.to_vec(),
            leyline(),
        ),
        &two,
    );
}

/// Pair 34: Grafdigger's Cage. Will can't recast Oracle from the graveyard,
/// but a land sacrificed to Edge of Autumn can still be replayed from there.
#[test]
fn cage_blocks_graveyard_casts_but_not_land_plays() {
    let cage = || board(&[Card::GrafdiggersCage]);
    let led_board = || {
        vec![
            Permanent::untapped(Card::LionsEyeDiamond),
            Permanent::untapped(Card::Swamp),
            Permanent::untapped(Card::Swamp),
            Permanent::untapped(Card::Swamp),
        ]
    };
    let two = [Card::Island, Card::Island];
    assert_fails(
        puzzle_vs(
            vec![Card::YawgmothsWill, Card::ThassasOracle],
            led_board(),
            two.to_vec(),
            cage(),
        ),
        &two,
    );
    // Edge of Autumn cycles away a tapped Island; Will replays it untapped
    // from the graveyard for Oracle's second blue.
    let board_with_tapped_island = || {
        vec![
            Permanent::tapped(Card::Island),
            Permanent::untapped(Card::Island),
            Permanent::untapped(Card::Swamp),
            Permanent::untapped(Card::Swamp),
            Permanent::untapped(Card::Swamp),
        ]
    };
    let one = [Card::Plains];
    let steps = assert_wins_vs(
        puzzle_vs(
            vec![Card::YawgmothsWill, Card::ThassasOracle, Card::EdgeOfAutumn],
            board_with_tapped_island(),
            one.to_vec(),
            cage(),
        ),
        &one,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::PlayLand {
            card: Card::Island,
            from: Zone::Graveyard,
        }
    )));
    assert_fails(
        puzzle_vs(
            vec![Card::ThassasOracle, Card::EdgeOfAutumn],
            board_with_tapped_island(),
            one.to_vec(),
            cage(),
        ),
        &one,
    );
}

/// Pair 35: Thalia. A line exactly on mana for Brainstorm fails; one more
/// land wins; her tax also prices the opponent out of Counterspell.
#[test]
fn thalia_taxes_the_cantrip_and_the_counter() {
    let thalia = || board(&[Card::Thalia]);
    let pile = [Card::ThassasOracle, Card::Swamp, Card::Plains];
    assert_wins(
        puzzle(vec![Card::Brainstorm], islands(3), pile.to_vec()),
        &pile,
    );
    assert_fails(
        puzzle_vs(vec![Card::Brainstorm], islands(3), pile.to_vec(), thalia()),
        &pile,
    );
    assert_wins_vs(
        puzzle_vs(vec![Card::Brainstorm], islands(4), pile.to_vec(), thalia()),
        &pile,
    );
    // Counterspell on two Islands costs three under Thalia.
    let one = [Card::Swamp];
    let mut taxed = thalia();
    taxed.hand = vec![Card::Counterspell];
    taxed.battlefield.extend(islands(2));
    assert_wins_vs(
        puzzle_vs(vec![Card::ThassasOracle], islands(2), one.to_vec(), taxed),
        &one,
    );
}

/// Pair 36: Archon of Emeria. Doomsday was your spell, so only abilities win:
/// Doctor Doom already in play draws the last card.
#[test]
fn archon_leaves_only_abilities() {
    let archon = || board(&[Card::ArchonOfEmeria]);
    let one = [Card::Swamp];
    assert_fails(
        puzzle_vs(
            vec![Card::ThassasOracle],
            islands(2),
            one.to_vec(),
            archon(),
        ),
        &one,
    );
    // Doctor Doom already in play draws the last card with an ability.
    let steps = assert_wins_vs(
        puzzle_vs(
            vec![Card::ThassasOracle],
            vec![Permanent::untapped(Card::DoctorDoom)],
            one.to_vec(),
            archon(),
        ),
        &one,
    );
    assert!(has_action(&steps, |a| matches!(a, Action::ActivateDoom)));
    assert!(!has_action(&steps, |a| matches!(
        a,
        Action::CastSpell { .. }
    )));
}

// --------------------------------------------------------------- stage 6a

/// Pair 37: Night's Whisper digs two cards deep, but the 2 life kills you if
/// Doomsday left you at 2.
#[test]
fn nights_whisper_needs_the_life_to_spare() {
    let pile = [Card::ThassasOracle, Card::Swamp, Card::Plains];
    let lands = || {
        vec![
            Permanent::untapped(Card::Swamp),
            Permanent::untapped(Card::Island),
            Permanent::untapped(Card::Island),
            Permanent::untapped(Card::Island),
        ]
    };
    let mut fine = puzzle(vec![Card::NightsWhisper], lands(), pile.to_vec());
    fine.you.life = 6;
    let steps = assert_wins(fine, &pile);
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::NightsWhisper,
            ..
        }
    )));
    let mut dead = puzzle(vec![Card::NightsWhisper], lands(), pile.to_vec());
    dead.you.life = 4;
    assert_fails(dead, &pile);
}

/// Pair 38: Manamorphose turns red and white mana into the {U}{U} Oracle needs.
#[test]
fn manamorphose_fixes_the_colors_for_oracle() {
    let one = [Card::Swamp];
    let lands = || {
        vec![
            Permanent::untapped(Card::Mountain),
            Permanent::untapped(Card::Plains),
        ]
    };
    let steps = assert_wins(
        puzzle(
            vec![Card::Manamorphose, Card::ThassasOracle],
            lands(),
            one.to_vec(),
        ),
        &one,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::Choose(doomdle::Choice::AddMana(mana)) if mana.get(doomdle::Color::U) == 2
    )));
    assert_fails(
        puzzle(vec![Card::ThassasOracle], lands(), one.to_vec()),
        &one,
    );
}

/// Pair 39: Gush is free after tapping the two Islands it returns, and draws
/// into the win.
#[test]
fn gush_floats_the_mana_and_draws_two() {
    let pile = [Card::ThassasOracle, Card::Swamp];
    let steps = assert_wins(puzzle(vec![Card::Gush], islands(2), pile.to_vec()), &pile);
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::Gush,
            payment: doomdle::Payment {
                alt: Some(doomdle::AltCost::ReturnTwoIslands(..)),
                ..
            },
            ..
        }
    )));
    // One Island: no alternative cost, no five mana.
    assert_fails(puzzle(vec![Card::Gush], islands(1), pile.to_vec()), &pile);
}

/// Pair 40: Reckless Impulse isn't a draw, so Narset can't stop it finding
/// Oracle.
#[test]
fn reckless_impulse_gets_around_narset() {
    let pile = [Card::ThassasOracle, Card::Swamp, Card::Plains];
    let narset = || board(&[Card::Narset]);
    let lands = || {
        vec![
            Permanent::untapped(Card::Mountain),
            Permanent::untapped(Card::Island),
            Permanent::untapped(Card::Island),
            Permanent::untapped(Card::Island),
        ]
    };
    assert_fails(
        puzzle_vs(vec![Card::Brainstorm], lands(), pile.to_vec(), narset()),
        &pile,
    );
    let steps = assert_wins_vs(
        puzzle_vs(
            vec![Card::RecklessImpulse],
            lands(),
            pile.to_vec(),
            narset(),
        ),
        &pile,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::CastSpell {
            card: Card::ThassasOracle,
            from: Zone::Exile,
            ..
        }
    )));
}

/// Pair 41: Timetwister with exactly seven cards draws them all; eight leaves
/// the order unknown.
#[test]
fn timetwister_needs_exactly_seven() {
    // Hand after casting: Oracle. Graveyard: Doomsday. Library: the pile.
    // 1 + 1 + 5 = 7, and Oracle comes back with devotion for an empty library.
    let pile = [
        Card::Swamp,
        Card::Plains,
        Card::Mountain,
        Card::Forest,
        Card::Tundra,
    ];
    let lands = || {
        let mut v = islands(5);
        v.push(Permanent::untapped(Card::Swamp));
        v
    };
    assert_wins(
        puzzle(
            vec![Card::Timetwister, Card::ThassasOracle],
            lands(),
            pile.to_vec(),
        ),
        &pile,
    );
    // One more card in hand makes eight. A land with no land drop left
    // can't be shed (a cantrip could rearrange the count, as the solver
    // happily found with Brainstorm).
    let mut eight = puzzle(
        vec![Card::Timetwister, Card::ThassasOracle, Card::Mountain],
        lands(),
        pile.to_vec(),
    );
    eight.you.land_drop_available = false;
    assert_fails(eight, &pile);
}

/// Pair 42: Sol Ring pays Laboratory Maniac's generic mana but can't pay
/// Oracle's {U}{U}.
#[test]
fn sol_ring_pays_generic_only() {
    let one = [Card::Swamp];
    let board_with_ring = || {
        vec![
            Permanent::untapped(Card::SolRing),
            Permanent::untapped(Card::Island),
        ]
    };
    let steps = assert_wins(
        puzzle(
            vec![
                Card::LaboratoryManiac,
                Card::GitaxianProbe,
                Card::GitaxianProbe,
            ],
            board_with_ring(),
            one.to_vec(),
        ),
        &one,
    );
    assert!(has_action(&steps, |a| matches!(
        a,
        Action::TapForColorless {
            source: Card::SolRing
        }
    )));
    assert_fails(
        puzzle(vec![Card::ThassasOracle], board_with_ring(), one.to_vec()),
        &one,
    );
}
