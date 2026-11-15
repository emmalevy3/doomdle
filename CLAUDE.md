# Doomdle

A daily Wordle-style Magic: The Gathering puzzle. The player sees a game state (hand, board, mana, life, library/graveyard, an open-information opponent) and must build a Doomsday pile that wins this turn. This repo is the Rust rules engine and solver that validates submitted piles. It will later compile to WebAssembly for a static web page.

## Commands
- Build: `cargo build`
- Run: `cargo run`
- Test all: `cargo test`
- Single test: `cargo test <test_name_substring>` (add `-- --exact` to match a full path like `module::tests::name`)
- Lint: `cargo clippy --all-targets -- -D warnings`
- Format: `cargo fmt` (check only: `cargo fmt --check`)

Before saying a task is done, run fmt, clippy and test and make sure all three pass. Report the test results in your summary.

## Environment
Development happens inside the dev container in `.devcontainer/` (image `mcr.microsoft.com/devcontainers/rust:1`, with Node and Claude Code installed). Only the project folder is mounted; Claude Code config persists in a Docker volume at `/home/vscode/.claude`.

## Project state
Rust crate, edition 2024, no dependencies yet. Logic lives in a library crate so it can later compile to WebAssembly; `src/main.rs` stays a thin CLI for running puzzles locally. Update this section as structure emerges.

Stages 1–5, 6a and 6b are done. Modules:
- `src/mana.rs` — `Color`, `ManaPool` (per-color counts + colorless), `ManaCost` (colored, Phyrexian, {R/G} hybrid and generic pips), `Payment` (mana plus life, an optional `AltCost`: pitch a card, return one or two Islands, and a `replicate` count), and `Payment::options` (every distinct way to pay a mana cost, including 2 life per Phyrexian pip, capped by your life total).
- `src/cards.rs` — the `Card` enum and all card behavior as `match` arms: costs, kinds, land subtypes, mana abilities, cycling costs, fetch filters, enter triggers, loyalty abilities (`loyalty_abilities(state, player)`, with Teferi's −3 targets), the other activated abilities (`activated_abilities(state, player)`: Aether Spellbomb's bounce and draw, Mishra's Bauble, equip, each with its mana cost and timing), dies triggers (`dies_trigger`: Chromatic Star and Wizard's Rockets draw when they really reach the graveyard), color predicates (`is_blue` etc.), `replicate_cost`/`has_storm`/`evoke_pitch`, `target_options` (graveyard cards, the other player's stack items, the other player's permanents; Veil of Summer's "can't be countered" and hexproof and Lightning Greaves' shroud are applied in `permanent_targets` and re-checked on resolution by `find_target`, which picks the first copy that may still be targeted), `alternative_payments` (Force of Will, Force of Negation, Daze, Snuff Out, Mindbreak Trap, Grief's evoke), `resolve_spell`/`resolve_trigger`/`resolve_ability`, `toughness`, the shared `counter`, `tax`, `remove_permanent`, `bounce`, `surgical` and `discard_from` helpers, the `Copies` trigger that makes storm/replicate copies, Bowmasters' ping trigger (nothing to ping under Veil), Skateboard's tap trigger, Unearth under Grafdigger's Cage, and the options and application of player choices (Brainstorm put-back, Ponder order, scry, surveil, land search, Flow State, Frantic Search's discard and untap, paying or declining a counter's tax, spreading copies over targets, picking a card from the opponent's hand, aiming Bowmasters' ping, choosing Manamorphose's colors, aiming Skateboard's tap).
- `src/state.rs` — `State` (both players, stack, priority, pending choice, outcome), `PlayerState` (zones, mana, counters, and the turn-long flags `yawgmoths_will`, `silenced`, `veiled`, `sorceries_have_flash`, `cast_blue_or_black_this_turn`), `Permanent` (tapped, summoning sick, loyalty, damage, and which equipment the creature wears: `greaves` gives haste and shroud, `skateboard` haste; `can_tap_for_ability` for Doom), `StackItem` (spell with its target and `SpellMode` normal/copy/evoked, trigger, activated ability), `Target` (a card in a graveyard, a stack index plus the card there, a permanent, or a player), `Zone` (hand, graveyard, or the cards Reckless Impulse exiled into `impulse`), `Puzzle`, `Outcome`, `draw` (with the Lab Man/Jace replacement, the `drew_from_empty` flag that loses at the next state-based check, Narset's one-draw cap and Bowmasters' ping triggers), `enter_battlefield` (nonbasics enter tapped under the other player's Archon), `PlayerState::take_permanent` (the one way off the battlefield; equipment that leaves comes off its wearer), `State::battlefield_to_graveyard` (destroy or sacrifice: leave, bin, and queue the dies trigger only if the card really landed in the graveyard), `State::put_in_graveyard` and `State::mill` (the one door into a graveyard, so Yawgmoth's Will and the other player's Leyline of the Void can exile instead), `enter_battlefield` also asks for Skateboard's trigger target before the trigger goes on the stack, `controls`, state-based checks (your life, your empty draw, the opponent at 0 life, creatures with lethal damage), and `normalize` (sorts hand/graveyard/exile/battlefield so equivalent states hash equal). `Display` for all of them.
- `src/engine.rs` — `Action`, `Choice`, `legal_actions`, `apply`. `CastSpell` and `PlayLand` say which `Zone` the card comes from (the graveyard only under Will) and `CastSpell` carries the target for targeted spells. The opponent's actions (`their_actions`) are only targeted instants from hand aimed at your things, plus pass; nothing at all while Silenced or while you control Teferi; their lands are tapped as part of the cast (`reachable_pools`/`tap_plan`/`pay_mana`), never on their own, and `extra_generic` adds Defense Grid's {3} per Grid to their payments and Thalia's {1} per Thalia to everyone's noncreature spells in `cast_payments`. `push_plays` handles timing and restrictions: Teferi makes the other player's instants sorcery-speed and his +1 gives the controller's sorceries flash, Archon of Emeria caps each player at one spell a turn (`spell_limit_reached`), and Grafdigger's Cage forbids casting from the graveyard (lands may still be played). Casting a storm or replicated spell also pushes a `Copies` trigger; an evoked creature is sacrificed as it enters in `resolve`. `remove_stack_item` keeps stack targets pointing at the right item and clears a target on the removed item so that spell fizzles (a targeted spell with `target: None` does nothing on resolution); copies that resolve or are countered go to no zone. A resolution that needs a decision sets `State::pending`; until it is answered the only legal actions are `Action::Choose`, a choice may queue a follow-up choice (Frantic Search), and the decider may differ from the spell's owner (`PendingChoice::bin_owner`, for "unless its controller pays"). Activated abilities (cycling, fetchlands, Doom's tap, loyalty, and `Action::Activate` for Spellbomb, Bauble and equip, which is sorcery-speed) pay their costs on activation and put a `StackItem::Ability` on the stack; mana abilities never touch the stack (`TapForMana`, `TapForColorless` for Sol Ring, `SacrificeForMana`, `ActivateLed`, `FilterMana` for Chromatic Star and Wizard's Rockets' X). Puzzles start in `Phase::ChoosingPile`, where the only actions are `Action::ChoosePile`.
- `src/solver.rs` — `solve` (AND/OR DFS, memoized on `State`), `Strategy` with a `walk`/`line`/`outcomes` replay (`line` only when the opponent never branches), and `verify(puzzle, pile) -> Verdict::{Wins, Fails}` as the single entry point for checking a submitted pile.
- `tests/solver_piles.rs` — winning / near-miss pile pairs, including pairs against an opponent holding a counter.

Puzzles are built as `Puzzle` structs in code; there is no file format yet.

## Rules source of truth
- `CARDS.md` defines every card in the pool and exactly how it behaves *in this engine*. Read it before touching any card logic.
- Never add a card that isn't in `CARDS.md`. Never change `CARDS.md` yourself; propose the change and I'll make it.
- If a card interaction isn't covered by `CARDS.md` or the scope below, stop and ask. Do not guess Magic rules.

## Scope
- The puzzle starts as Doomsday resolves: the player chooses 5 cards from library + graveyard and orders them.
- One turn only, no combat, no opponent turn.
- The opponent can respond (counterspells, Stifle, etc.) but never acts proactively. See "Decisions" in `CARDS.md` for the exact rules.
- Mana is tracked as simple per-color counts (plus colorless), not a general mana system.

## Architecture
- `Card` is an enum with one variant per card. All card-specific behavior lives in one module (`src/cards.rs`) as `match` arms.
- `State` is a plain data struct deriving `Clone, Debug, PartialEq, Eq, Hash`. It covers both players.
- The engine has a real stack and priority from the start, even while the opponent always passes. Spells and triggers go on the stack; both players get priority before the top item resolves. Do not resolve spells immediately as a shortcut.
- `apply(&State, Action) -> State` is pure: it returns a new state and never mutates its input.
- `legal_actions(&State) -> Vec<Action>` is the only place that decides what's legal, for whichever player has priority or a choice.
- The solver is an AND/OR depth-first search: at your decision points it needs one winning action; at the opponent's it needs every action to still lose for them. It returns your winning strategy if one exists. Memoize visited states.
- No `unwrap()`/`expect()` outside tests; return `Result`/`Option`.
- Keep dependencies minimal. Ask before adding a crate.

## Implementation order
Work one stage at a time; each stage ends with passing tests.
1. Core: `State`, stack and priority (opponent always passes), the AND/OR solver, Doomsday's pile choice, Thassa's Oracle, the blue cantrips, LED, Lotus, Petal, Moxen, Dark Ritual, basics and original duals.
2. Costs and library order: cycling, life payments, fetchlands, the unordered-library rule, surveil lands, the other win conditions.
3. Graveyard: Unearth, Yawgmoth's Will, Flow State, Cabal Ritual, Frantic Search.
4. Opponent responses, in three parts:
   - 4a (done): opponent priority and the counterspells — Counterspell, Force of Will, Force of Negation, Daze, Spell Pierce, Miscalculation, Mental Misstep, Spell Snare, Stifle, Pact of Negation.
   - 4b (done): removal and the odd counters — Pyroblast, Hydroblast, Swords to Plowshares, Snuff Out, Surgical Extraction, Flusterstorm, Consign to Memory, Mindbreak Trap.
   - 4c (done): your protection and discard — Silence, Veil of Summer, Teferi, Defense Grid, Thoughtseize, Duress, Grief.
5. Opponent's board: the static hate permanents (done): Narset, Orcish Bowmasters, Leyline of the Void, Grafdigger's Cage, Thalia, Archon of Emeria.
6. Everything else in `CARDS.md`, in three parts:
   - 6a (done): draw and mana spells — Night's Whisper, Meditate, Ideas Unbound, Manamorphose, Gush, Timetwister, Reckless Impulse, Sol Ring, Gleemox.
   - 6b (done): artifact abilities and equipment — Aether Spellbomb, Chromatic Star, Wizard's Rockets, Mishra's Bauble, Lightning Greaves, Skateboard.
   - 6c: creatures and Cavern — Quantum Riddler, Tamiyo, Thundertrap Trainer, Cavern of Souls.

## Testing
- Every card gets unit tests for each behavior listed in `CARDS.md`, including its costs (mana and life).
- Solver tests live in `tests/` and come in pairs: a known-winning pile that must be found, and a near-miss (a mana short, a life short, too little devotion, wrong order) that must fail.
- Once opponent responses exist, add pairs where a pile wins against a passive opponent but loses to the opponent's hand, and a pile that plays around it.
- Never weaken or delete a failing test to make it pass. Tell me instead.

## Working with me
- I'm learning Rust. In your summary, briefly explain any Rust feature or idiom I might not know (ownership/borrowing choices, trait derives, pattern matching tricks).
- Keep changes scoped to the task. Mention anything else you noticed instead of fixing it unasked.