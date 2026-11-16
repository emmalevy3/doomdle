// Smoke test for the WebAssembly build. Run after
// `wasm-pack build --target web --out-dir web/pkg --features wasm`:
//
//     node wasm/smoke.mjs
//
// Loads the package the way a browser would (the `web` target), then checks
// a winning pile, a losing pile, a bad card name and the solver on the
// sample puzzles, the failure feedback, the worker's request handler and the
// sentences the page shows. Exits non-zero on the first mismatch.

import { readFileSync } from "node:fs";
import init, * as api from "../web/pkg/doomdle.js";
import { handleRequest } from "../web/verify.js";
import { verdictSentence, lifeAfterDoomsday, localDate, listNames } from "../web/describe.js";

const { verify_pile, solve_puzzle } = api;

const wasmBytes = readFileSync(new URL("../web/pkg/doomdle_bg.wasm", import.meta.url));
await init({ module_or_path: wasmBytes });

const puzzle = (name) =>
  readFileSync(new URL(`../web/puzzles/${name}.json`, import.meta.url), "utf8");

let failures = 0;
function check(label, actual, expected) {
  const ok = Object.entries(expected).every(
    ([key, value]) => JSON.stringify(actual[key]) === JSON.stringify(value),
  );
  console.log(`${ok ? "ok  " : "FAIL"} ${label}`);
  if (!ok) {
    failures += 1;
    console.log(`     got ${JSON.stringify(actual)}`);
  }
}

const cavern = puzzle("cavern-wizard");
const led = puzzle("led-brainstorm");

check(
  "winning pile wins",
  JSON.parse(verify_pile(cavern, JSON.stringify(["Swamp"]))),
  { ok: true, wins: true, gave_up: false },
);
check(
  "Oracle at the bottom loses",
  JSON.parse(
    verify_pile(
      led,
      JSON.stringify([
        "Ancestral Recall",
        "Lotus Petal",
        "Mox Sapphire",
        "Black Lotus",
        "Thassa's Oracle",
      ]),
    ),
  ),
  { ok: true, wins: false, gave_up: false },
);
check(
  "a misspelled card is an error",
  JSON.parse(verify_pile(cavern, JSON.stringify(["Swmap"]))),
  { ok: false },
);
check(
  "malformed puzzle is an error",
  JSON.parse(verify_pile("{", "[]")),
  { ok: false },
);
check(
  "solver finds the pile",
  JSON.parse(solve_puzzle(cavern)),
  { ok: true, pile: ["Swamp"], gave_up: false },
);

const winning = JSON.parse(verify_pile(cavern, JSON.stringify(["Swamp"])));
if (!String(winning.line).includes("You win.")) {
  failures += 1;
  console.log("FAIL the winning line ends in a win");
}

// Feedback on failures.
const wrongOrder = JSON.parse(
  verify_pile(
    led,
    JSON.stringify([
      "Ancestral Recall",
      "Lotus Petal",
      "Mox Sapphire",
      "Black Lotus",
      "Thassa's Oracle",
    ]),
  ),
);
check("a failed pile gets a closest line", wrongOrder.feedback?.closest ?? {}, {
  cards_left: wrongOrder.feedback?.closest?.cards_left,
});
if (!wrongOrder.feedback?.closest?.line) {
  failures += 1;
  console.log("FAIL the closest line has steps");
}
const plain = cavern.replace(/\{"card": "Cavern of Souls", "type": "Wizard"\}/, '"Island"');
const countered = JSON.parse(verify_pile(plain, JSON.stringify(["Swamp"])));
check("Counterspell is named", countered.feedback ?? {}, {
  wins_without_their_hand: true,
  stopped_by: ["Counterspell"],
});
check(
  "the sentence names Counterspell",
  { sentence: verdictSentence(countered) },
  { sentence: "This pile wins unless they have Counterspell." },
);

// The worker's handler.
check(
  "handleRequest verifies",
  handleRequest(api, { id: 7, kind: "verify", puzzle: cavern, pile: ["Swamp"] }),
  { id: 7, result: JSON.parse(verify_pile(cavern, JSON.stringify(["Swamp"]))) },
);
check(
  "handleRequest rejects unknown kinds",
  handleRequest(api, { id: 8, kind: "nope", puzzle: cavern }).result,
  { ok: false },
);

// Sentences and helpers.
check(
  "sentences",
  {
    win: verdictSentence({ ok: true, wins: true }),
    gaveUp: verdictSentence({ ok: true, wins: false, gave_up: true }),
    empty: verdictSentence({
      ok: true,
      wins: false,
      gave_up: false,
      feedback: { closest: { cards_left: 0, ended: "you drew from an empty library" } },
    }),
    ranOut: verdictSentence({
      ok: true,
      wins: false,
      gave_up: false,
      feedback: { closest: { cards_left: 2, ended: "the turn ends without a win" } },
    }),
    together: verdictSentence({
      ok: true,
      wins: false,
      gave_up: false,
      feedback: { wins_without_their_hand: true, stopped_by: ["Daze", "Force of Will"] },
    }),
  },
  {
    win: "This pile wins.",
    gaveUp: "The check ran out of time on this pile, so it doesn't count as an attempt.",
    empty: "Closest line: you drew from an empty library.",
    ranOut: "Closest line: you ran out of plays with 2 cards left in your library.",
    together: "This pile wins unless they have Daze and Force of Will together.",
  },
);
check(
  "helpers",
  {
    life20: lifeAfterDoomsday(20),
    life7: lifeAfterDoomsday(7),
    date: localDate(new Date(2026, 0, 5)),
    names: listNames(["A", "B", "C"]),
  },
  { life20: 10, life7: 3, date: "2026-01-05", names: "A, B and C" },
);

if (failures > 0) {
  console.error(`${failures} check(s) failed`);
  process.exit(1);
}
console.log("all checks passed");
