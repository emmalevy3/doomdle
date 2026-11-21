# Handoff: how Doomdle works and where it stands

Written for whoever (or whichever model) continues this work. Read `CLAUDE.md`
first (rules and module map), then this file, then `CARDS.md` before touching
any card logic. Everything here is current as of 2026-11-21: stages 1–12 plus
the post-launch work listed under "Since launch" below.

## Status in one paragraph

Every card in `CARDS.md` is implemented, An Offer You Can't Refuse included
(96 `Card` variants: basics and duals are listed individually, Tamiyo's back
face is its own variant, and so is the Treasure token). The engine, AND/OR
solver and a JSON puzzle format are done. `cargo test` runs 351 tests: 278
unit, 72 solver pile pairs in `tests/solver_piles.rs`, 1 corpus test in
`tests/puzzle_files.rs` over `web/puzzles/*.json`. `cargo fmt --check`, both
clippy runs (with and without `--features wasm`) and the tests are all green.
Dependencies: `serde` (derive), `serde_json`, and `wasm-bindgen` behind the
`wasm` feature. Stage 7 put the verifier in the browser: `wasm-pack build
--target web --out-dir web/pkg --features wasm` builds `web/pkg/`, and `node
wasm/smoke.mjs` checks it. Stage 8 added the site in `web/` and failure
feedback; stage 9 restyled it (dark blue-black Magic look) and laid it out from
the user's sketch. Stage 10 added puzzle ratings (`src/quality.rs`) and a
generator (`src/generate.rs`, the `rate`/`survey`/`generate` CLI commands).
Stage 11 scheduled daily puzzles and stage 12 launched the site on
emmanems.com/doomdle (CI deploys `main`). Puzzles are mostly generated and
scheduled; hand-made ones (and guest ones, see below) replace a dated file.

## How a game is represented

- `State` (`src/state.rs`) is plain data: `phase` (`ChoosingPile` then
  `Main`), `priority`, `passed_in_succession`, `stack: Vec<StackItem>`,
  `pending: Option<PendingChoice>`, `you`/`opponent: PlayerState`, `outcome`.
  It derives `Clone, PartialEq, Eq, Hash` so the solver can memoize on it;
  `normalize()` sorts hand, graveyard, exile, impulse and battlefield after
  every action so equivalent states hash equal. Because the battlefield is
  sorted and `Permanent` derives `Ord` with `card, tapped, summoning_sick, ...`
  first, "the first permanent named X" is always the untapped, unsick one.
- `PlayerState`: zones (`hand`, `library` with index 0 on top, `graveyard`,
  `exile`, `impulse` for Reckless Impulse), `library_ordered` (false after a
  shuffle; any later order-dependent step loses with
  `LossReason::LibraryUnordered`, see CARDS.md "Shuffles"), `mana_pool`,
  turn counters (`spells_cast_this_turn`, `cards_drawn_this_turn`),
  `land_drop_available`, and turn-long flags (`yawgmoths_will`, `silenced`,
  `veiled`, `sorceries_have_flash`, `cast_blue_or_black_this_turn`).
- `Permanent`: `card`, `tapped`, `summoning_sick`, `loyalty`, `loyalty_used`,
  `damage`, equipment flags on the wearer (`greaves` = haste + shroud,
  `skateboard` = haste), `token` (offspring copy or Treasure, ceases to exist
  on leaving; `Permanent::untapped(Card::Treasure)` is always a token),
  `chosen_type` (Cavern of Souls). Constructors: `untapped`, `tapped`,
  `entering`, `cavern(type)`.
- `StackItem`: `Spell { card, controller, target, mode }`, `Trigger { source,
  kind, controller }`, `Ability { source, kind, controller }`. `SpellMode` is
  `Normal`, `Copy` (storm/replicate copy, goes nowhere), `Evoked`, or
  `With { offspring, uncounterable }` (Trainer's offspring, Cavern mana).
- `Target`: `Graveyard { owner, card }`, `Stack { index, card }`,
  `Permanent { owner, card }`, `Player`. Targets are fixed when cast.
  `remove_stack_item` keeps stack indices right and clears a target that
  pointed at the removed item; a targeted spell with `target: None` fizzles.
- `Puzzle { you, opponent }` is the state as Doomsday resolves. `you.library`
  is the *candidate set* for the pile; `State::new(puzzle)` starts in
  `ChoosingPile`, where the only actions are `ChoosePile(Vec<Card>)`.
  `choose_pile` exiles the rest, halves your life (rounded up), counts
  Doomsday as a spell cast, bins Doomsday.

## The engine loop (`src/engine.rs`)

- `legal_actions(&State) -> Vec<Action>` is the only legality source. If the
  game is over: nothing. In `ChoosingPile`: every ordered selection of up to 5
  candidates. If `pending` is set: only `Action::Choose(..)` from
  `cards::choice_options`. Otherwise `your_actions` or `their_actions`.
- `apply(&State, &Action) -> Result<State, EngineError>` is pure: clones,
  checks `legal_actions` contains the action, dispatches, then
  `check_state_based()` (your life ≤ 0, drew from empty, opponent at 0 life =
  you win, lethal damage on creatures) and `normalize()`.
- Priority: a non-pass action keeps priority (`acted`); `Pass` flips
  priority; two passes in a row (`passed_in_succession`) resolve the top of
  the stack via `resolve`, or end the turn (a loss) on an empty stack.
- Your actions, in order (the solver tries them in this order): Pass, casts
  and land plays from hand / graveyard (under Will) / impulse, cycling,
  fetchland and Doom activations, loyalty abilities (sorcery speed, loyalty
  checked), `Activate` for Spellbomb/Bauble/equip, then mana abilities
  (`TapForMana`, `TapForColorless`, `SacrificeForMana`, `ActivateLed`,
  `FilterMana` for Chromatic Star / Wizard's Rockets). Mana abilities never
  touch the stack.
- `push_plays`/`push_casts`: timing (instants any time; sorceries, creatures,
  artifacts, enchantments, planeswalkers only with an empty stack; Teferi
  makes the other player's instants sorcery-speed; his +1 gives your sorceries
  flash), Archon's one-spell limit, Grafdigger's Cage (no casting from the
  graveyard), then one `CastSpell` per payment per target.
- `cast_payments`: every way to pay from the pool (`Payment::options` handles
  Phyrexian life and `{R/G}` hybrid), replicate repeats, offspring (+{4}),
  alternative costs from `Card::alternative_payments` (pitch, Daze's Island,
  Gush's two Islands, Snuff Out's life, Mindbreak Trap free, warp) each with
  its own base cost, plus `extra_generic` (Defense Grid ×3 for the opponent,
  Thalia ×1 for noncreature spells). For a creature spell matching a Cavern's
  chosen type, pools are extended by any-color mana from untapped Caverns and
  the payment records `cavern: k`; such a spell is uncounterable.
- The opponent (`their_actions`): only targeted instants from their open hand
  aimed at your stack items / permanents / graveyard, plus Pass. Their lands
  are tapped *as part of casting* (`reachable_pools`, `tap_plan`, `pay_mana`),
  never on their own; they never float mana, cycle, play lands or cast
  untargeted spells. Sacrifice sources count too: Treasures (which they only
  get when you Offer one of their spells) are sacrificed as part of paying,
  and `tap_plan` taps lands before sacrificing when both would do. Nothing at
  all while Silenced, under your Teferi, or once Archon's limit is hit. Counters are not even offered against spells
  that can't be countered (Veil, Cavern): same result, fewer branches.
- Funnels, so replacement effects apply everywhere:
  `State::put_in_graveyard` (Yawgmoth's Will / the other player's Leyline
  exile instead; returns whether it landed), `PlayerState::take_permanent`
  (the only way off the battlefield: clears equipment, drops tokens, turns
  Tamiyo's back face into the front), `State::battlefield_to_graveyard`
  (destroy/sacrifice + dies trigger only if it really reached the graveyard),
  `State::enter_battlefield` (Archon's enters-tapped, enter triggers, and the
  pending choices for Skateboard's target and Cavern's type),
  `State::draw` (Lab Man/Jace win on an empty draw, otherwise
  `drew_from_empty` loses at the next state-based check; Riddler's extra
  card when the hand is small; Narset's one-draw cap; Bowmasters' ping per
  card; Tamiyo's transform on the third card).
- Pending choices (`ChoiceKind` in state.rs, options and application in
  `cards::choice_options` / `cards::apply_choice`): the resolution pauses,
  the decider may differ from the spell's owner (`PendingChoice::bin_owner`,
  used by "unless its controller pays"), a choice may queue another
  (Frantic Search), and `bin` is the spell binned once the choice is made.

## Cards (`src/cards.rs`)

Everything card-specific is a `match` arm on `Card`: `name`, `kind`,
`mana_cost`, `toughness`, `creature_types`, color predicates (`is_blue`
etc., pips plus color indicators for Pact and Tamiyo's back face),
`land_types`, `enters_tapped`, `fetch_filter`, `cycling_cost`,
`mana_ability`, `mana_colors`, `etb_trigger`, `dies_trigger`,
`starting_loyalty`, `loyalty_abilities`, `activated_abilities`,
`target_options`, `alternative_payments`, `warp_cost`, `offspring_cost`,
`replicate_cost`, `has_storm`, `evoke_pitch`, `castable`, `front_face`,
`is_equipment`; then `resolve_spell`, `resolve_trigger`, `resolve_ability`,
`choice_options`, `apply_choice`, and helpers (`counter`, `offer` (An Offer
You Can't Refuse: counter, then two Treasures for the target's controller even
if the counter fails), `tax`,
`remove_permanent`, `bounce`, `surgical`, `discard_from`, `push_copies`,
`permanent_targets`/`find_target` which apply Veil's hexproof and Greaves'
shroud both when offering and when resolving). `Card::ALL`, `FromStr` and
manual serde impls make names round-trip for the file format.

Who may target what: counters and removal only ever target the other
player's things (CARDS.md Decisions §2), with two exceptions, both yours
only: An Offer You Can't Refuse at your own noncreature spells (even veiled
ones, for the Treasures), and Pyroblast/Hydroblast at your own blue/red spells
and permanents (e.g. your Quantum Riddler, as bait for your own Offer).

To add a card: add the variant, the name, the kind, the cost, then whichever
arms its text needs, tests for each CARDS.md behavior and cost, and a solver
pair. Never add a card that isn't in CARDS.md; never edit CARDS.md unasked
(propose, the user says yes, then edit and commit separately).

## Solver and file format

- `solve(&State)` (`src/solver.rs`): AND/OR depth-first search, memoized on
  `State` (`InProgress`/`Lost`/`Won`). Your nodes need one winning action;
  the opponent's need every action to lose. Returns a `Strategy` tree
  (`Mine`, `Theirs { branches }`, `Win`) that `walk`/`line`/`outcomes` can
  replay. `verify(puzzle, pile) -> Verdict::{Wins{start, strategy}, Fails}`;
  `solve_puzzle(puzzle)` tries every legal pile (fresh memo each).
- `src/puzzle_file.rs`: `PuzzleFile` / `PlayerFile` / `PermanentFile`, cards
  by printed name, `mana_pool` as a symbol string, optional `solution`,
  `attempts`, `difficulty` (`easy`/`medium`/`hard` from a rating, or
  `expert`, only ever set by hand) and `guest` (shown as "Idea by …" in the
  header, the Past puzzles list and the share text). A Treasure may only be
  on a battlefield; anywhere else is a parse error.
  Fields that can't be true as Doomsday resolves (exile, impulse, Silence,
  Veil, unordered library) deliberately don't exist. `parse`, `to_json`,
  `puzzle`, `from_puzzle`. Example files in `web/puzzles/`.
- CLI (`src/main.rs`): `cargo run` (sample), `cargo run -- web/puzzles/file.json` (search
  for a winning pile), `cargo run -- file.json "Card" ...` (verify a pile).

## Testing patterns

- Unit tests sit in each module. `state::test_support::{main_phase, versus}`
  build a main-phase state; `cards` tests have `resolve(state, card)`
  (push your spell, two passes), `resolve_targeting`, `with_trigger`,
  `with_ability`, `pass_twice`, `your_permanent`, `their_spell`, `on_stack`,
  `against(you, hate_cards)`. `engine` tests have `facing` (you cast a spell
  and passed to the opponent: note it already contains one pass, which
  shifts later pass counts by one), `their_cast`, `opponent_with`.
- Solver tests come in pairs: a winning pile that must be found and a
  near-miss that must fail. `assert_wins_vs` walks every opponent branch.
  Expect the solver to refute your near-miss designs: it has repeatedly found
  cheaper lines (cycling instead of casting, a land drop, Spellbomb's draw
  mode emptying the library). Design near-misses with that in mind.
- Zones are sorted by `Card` enum order, so expectations on hand/graveyard
  contents must be in enum order.

## Working agreements with the user

- One stage at a time, planned in plan mode and approved before coding.
- Before saying done: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`; report the test counts.
- The user is learning Rust: end each summary with short notes on Rust
  features or idioms used (ownership choices, derives, pattern tricks).
- Propose CARDS.md changes in the summary; after "yes", edit CARDS.md and
  commit that separately (`CARDS.md: document stage N simplifications`).
- Commit at the end of a stage (message ending with the Co-Authored-By trailer
  the harness asks for). The dev container has no git credentials: the user
  pushes from the host, so always say how many commits are ahead.
- No `unwrap`/`expect` outside tests; ask before adding a crate; never weaken
  or delete a failing test; keep changes scoped and mention other things
  noticed instead of fixing them.

## Known gaps and things noticed (not fixed)

- `activate_fetch` and `activate_doom` predate `activated_abilities` and could
  move onto it.
- Two Tamiyos ignore the legend rule; a puzzle starting at ≥3 draws with the
  front face in play never flips her.
- `solve_puzzle` uses a fresh memo per pile; the CLI ignores a file's
  `solution` when searching.
- The `Strategy` printout ends with a blank line.
- `Choice::Discard`'s doc still mentions Frantic Search only.
- Timetwister ignores the opponent's side (by CARDS.md decision).
- Search cost of Treasures and Lotus Petal: each is a five-way colour choice
  when sacrificed, so a busy puzzle with An Offer You Can't Refuse can leave
  most piles undecided at the rating's per-pile budget (today's 11/21 puzzle
  with the Offer added: 2,060 of 2,520 undecided, versus 192 without).
  Choosing the colour only when the mana is spent would fix it; not started.
  Keep Offer puzzles lean (a short hand) and run a site check before
  scheduling one.
- A Galaxy Z Fold player reported the phone "Add to pile" button sometimes
  stopped working until a refresh. The mis-taps were fixed with a bigger
  button; the "until refresh" part is unexplained (maybe the page switching
  between the phone and wide layouts near 1000 px when the Fold opens).

## Roadmap (proposed, not yet approved)

The user asked what remains to connect this to a website with daily puzzles
preloaded. The proposal, kept in the plan file as well:

7. **WebAssembly bridge** (done, see CLAUDE.md: `src/api.rs`, `src/wasm.rs`, the search budget, `wasm/smoke.mjs`). As proposed: install `wasm32-unknown-unknown` and
   `wasm-pack` (add to `.devcontainer`), add `wasm-bindgen` behind a `wasm`
   cargo feature and `crate-type = ["cdylib", "rlib"]`, a feature-gated
   `src/wasm.rs` exporting `verify_pile(puzzle_json, pile_json) -> json` and
   `solve(puzzle_json) -> json` as strings (errors as `{ok:false,error}`,
   never panics), a Verdict-to-JSON step in `puzzle_file.rs` shared with the
   CLI, a node budget in `solve` so a browser tab can't freeze, and a Node
   smoke script over the built `.wasm`. The library already has no
   filesystem, thread, clock or randomness use.
8. **The site** (done, see CLAUDE.md: `web/`, `src/feedback.rs`). Decided: site in this repo, plain files, 3 attempts adjustable per site and per puzzle, Scryfall images, failure feedback = which opponent card stops it + the closest line. Originally proposed: `web/` with Vite + TypeScript, no framework; views for the
   state, a pile builder, submit and verdict; card images via Scryfall by
   name; static hosting (GitHub Pages). Open product decisions: attempts per
   day, feedback on a failed pile (the engine exposes `LossReason` and the
   line), whether to reveal the winning line, share text.
9. **The site's look and layout** (done). The user rejected the first "dark
   ritual" pass (pentagram, "Begin the ritual") as too occult: keep it dark
   but make it feel like Magic. Layout follows the user's sketch (see
   CLAUDE.md, `web/`).
10. **Puzzle quality and a generator** (done). The "few answers vs under 1%"
    question was settled by play-testing: the share of winning piles tracks
    how hard a puzzle feels, so it now sorts puzzles into Easy/Medium/Hard
    (cutoffs 0.5%, 2%, 3%; over 3% is discarded: 2.86% felt easy but fine and 5.36% too easy in play-testing), stored as `difficulty` in
    the file and shown on the page and in the share text. Stage 11 should mix
    tiers across the week. The user also asked for: no floating mana in
    generated puzzles (easy to miss), no unpayable life-cost cards, a bigger
    life display and clearer land-drop wording; all done. The user's point: the
    challenge must be in choosing and ordering the pile, not in finding the
    line, since the solver plays the line. Proposed solver-measured quality
    checks: more candidates than slots (decoys), few winning piles among all
    legal piles, order matters, obvious piles fail. A Rust generator builds
    random states from the card pool and keeps the interesting ones for the
    user to review. Generated puzzles must use a full 5-card pile chosen from
    more candidates; hand-made ones may use fewer (the user can make small
    ones interesting by hand). The current samples are engine fixtures, not
    daily puzzles (`bauble-threshold` offers 4 cards for 4 slots, so there is
    nothing to choose).
11. **Daily puzzles** (done): `schedule` assigns generated puzzles to dates
    by the weekly mix (2 Easy, 3 Medium, 2 Hard; the user said roughly is
    fine), re-rating each, and writes `web/puzzles/YYYY-MM-DD.json`; the
    corpus test checks dated files for id, difficulty, repeats and gaps. The
    user's plan: launch with a few weeks, run the generator overnight a few
    more times (`generate --count 0`, restartable without repeats), then
    schedule the new batch. Original proposal for this stage, for reference: `puzzles/YYYY-MM-DD.json`, a bundling step
   that copies them into the site's static assets with `solution` stripped,
   the page picks today's date locally and fetches that file; authoring is
   write a file, `cargo run -- puzzles/<date>.json`, commit; the corpus test
   guards every file. A static site can't hide future puzzles from someone
   reading the bundle; only a backend could.
12. **Launch on emmanems.com** (in progress): Doomdle on Cloudflare Pages,
    deployed by `.github/workflows/ci.yml`; a router Worker in `site/` puts
    it at emmanems.com/doomdle beside the user's Emmadle (a separate
    Cloudflare Worker, plain static files, at /emmadle) and a landing page at
    the root. The user's steps: add emmanems.com to Cloudflare, switch the
    nameservers at Squarespace, create the Pages project, add the API token
    and account id as GitHub secrets, push. Emmadle's share link (in its own
    project's game.js) must change to https://emmanems.com/emmadle/.
    Original proposal (was 10): GitHub Actions running fmt, clippy, test, the
    wasm build and smoke test, then the Pages deploy on `main`. Also handle
    browser caching after a deploy (the dev server's stuck "Loading…" came
    from mixed cached and new modules), e.g. versioned file names.

## Since launch (2026-11-20 and 21)

- Hand-made puzzles: 2026-11-21 (Duress must take Swords, not Counterspell;
  Pyroblast answers the counter; Lab Man wins; one winning set, 10 orders)
  and 2026-11-22, a guest puzzle from the user's friend Venus (warp Quantum
  Riddler, Pyroblast your own Riddler and Offer the Pyroblast for two
  Treasures, then Oracle; Expert, "Idea by Venus"; one winning set, 12 of
  55,440 piles). The generated puzzles they replaced (`gen-1117-204`,
  `gen-1117-236`) are back in the scheduling pool.
- Designing hand-made puzzles, what we learned: `rate` gives the winning
  sets and orders; for a single pile, checking all 120 orders with the CLI is
  quick. Before scheduling, run every pile through `api::verify_json` (the
  site's own call, `DEFAULT_BUDGET`) to be sure none gives up; the rating's
  "undecided" uses a smaller budget. A scratch crate depending on this one by
  path did that (keep the thread count low: 28 parallel searches ran out of
  memory). Exactly one winning order needs every card to draw the next one;
  cards that draw two at once, outside draws usable any time, and Oracle
  winning with cards left (devotion) all add orders or filler sets. Two
  routes that each have one tight answer were tried at length (Oracle vs Lab
  Man, Jace vs Lab Man, against Counterspell/Swords/Stifle) and never worked:
  the cheaper route always has spare resources, so filler piles win too.
- Site: the rules text says Doomsday goes to your graveyard, and a small
  Doomsday sits under "Graveyard after Doomsday" (players miscounted
  Timetwister without it; the engine was right). Wide screens show the
  candidates in two columns; phones got a bigger "Add to pile" button.
- Engine: An Offer You Can't Refuse and Treasures; Pyroblast/Hydroblast on
  your own things; the opponent can pay with Treasures; Difficulty::Expert
  and the `guest` field.

Decisions made for stage 8: the site lives here under `web/`; three attempts
by default (`web/config.js`, overridable per puzzle); Scryfall images; hiding
future puzzles doesn't matter, but the page must open on today's puzzle.
