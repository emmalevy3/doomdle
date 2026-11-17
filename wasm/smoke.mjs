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
import {
  verdictSentence,
  lifeAfterDoomsday,
  localDate,
  listNames,
  shareRows,
  shareText,
  discordText,
} from "../web/describe.js";

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

// Sharing: one square per pile card, per counted attempt.
const miss = (left) => ({
  pile: ["A"],
  result: { ok: true, wins: false, gave_up: false, feedback: { closest: { cards_left: left } } },
});
const answered = {
  pile: ["A"],
  result: { ok: true, wins: false, gave_up: false, feedback: { stopped_by: ["Daze"] } },
};
const gaveUp = { pile: ["A"], result: { ok: true, wins: false, gave_up: true } };
const won = (line) => ({
  pile: ["Swamp", "Island"],
  result: { ok: true, wins: true, gave_up: false, line },
});
const share = { id: "2026-11-16", allowed: 3, pileSize: 5, link: "https://example.org/" };
check(
  "share rows",
  {
    rows: shareRows([miss(2), answered, gaveUp, won("x")], 5),
    short: shareRows([miss(1), won("x")], 3),
    noFeedback: shareRows([{ pile: [], result: { ok: true, wins: false, gave_up: false } }], 2),
  },
  {
    rows: ["🟥🟥🟥⬛⬛", "🟨🟨🟨🟨🟨", "🟩🟩🟩🟩🟩"],
    short: ["🟥🟥⬛", "🟩🟩🟩"],
    noFeedback: ["⬛⬛"],
  },
);
check(
  "share text",
  {
    won: shareText({ ...share, attempts: [miss(2), won("1. Swamp")] }),
    lost: shareText({ ...share, attempts: [miss(5), miss(4), miss(3)] }),
    discord: discordText({ ...share, attempts: [won("1. Play Swamp.")] }),
    discordLost: discordText({ ...share, attempts: [miss(5)] }),
    hard: shareText({ ...share, difficulty: "hard", attempts: [won("x")] }),
  },
  {
    won: "Doomdle 2026-11-16 2/3\n🟥🟥🟥⬛⬛\n🟩🟩🟩🟩🟩\nhttps://example.org/",
    lost: "Doomdle 2026-11-16 X/3\n⬛⬛⬛⬛⬛\n🟥⬛⬛⬛⬛\n🟥🟥⬛⬛⬛\nhttps://example.org/",
    discord: "Doomdle 2026-11-16 1/3\n🟩🟩🟩🟩🟩\nhttps://example.org/\nMy pile: ||Swamp → Island||",
    discordLost: "Doomdle 2026-11-16 X/3\n⬛⬛⬛⬛⬛\nhttps://example.org/",
    hard: "Doomdle 2026-11-16 (Hard) 1/3\n🟩🟩🟩🟩🟩\nhttps://example.org/",
  },
);

if (failures > 0) {
  console.error(`${failures} check(s) failed`);
  process.exit(1);
}
console.log("all checks passed");
