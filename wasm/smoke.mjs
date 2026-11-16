// Smoke test for the WebAssembly build. Run after
// `wasm-pack build --target web --features wasm`:
//
//     node wasm/smoke.mjs
//
// Loads the package the way a browser would (the `web` target), then checks
// a winning pile, a losing pile, a bad card name and the solver on the
// sample puzzles. Exits non-zero on the first mismatch.

import { readFileSync } from "node:fs";
import init, { verify_pile, solve_puzzle } from "../pkg/doomdle.js";

const wasmBytes = readFileSync(new URL("../pkg/doomdle_bg.wasm", import.meta.url));
await init({ module_or_path: wasmBytes });

const puzzle = (name) =>
  readFileSync(new URL(`../puzzles/${name}.json`, import.meta.url), "utf8");

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

if (failures > 0) {
  console.error(`${failures} check(s) failed`);
  process.exit(1);
}
console.log("all checks passed");
