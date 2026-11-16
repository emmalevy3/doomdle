// Loads today's puzzle, runs the attempts, and shows the feedback.

import { ATTEMPTS, REVEAL_ON_LOSS } from "./config.js";
import { renderBoard, setImages, setZoomDialog } from "./board.js";
import { verdictSentence, localDate } from "./describe.js";
import { createPileBuilder } from "./pile.js";
import { loadRecord, saveRecord } from "./storage.js";

const $ = (id) => document.getElementById(id);

// ---------------------------------------------------------------- verifier

const worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
const waiting = new Map();
let nextId = 0;
worker.addEventListener("message", (event) => {
  const resolve = waiting.get(event.data.id);
  waiting.delete(event.data.id);
  resolve?.(event.data.result);
});
worker.addEventListener("error", () => {
  for (const resolve of waiting.values()) {
    resolve({ ok: false, error: "the verifier failed to load" });
  }
  waiting.clear();
});

function ask(request) {
  return new Promise((resolve) => {
    const id = nextId++;
    waiting.set(id, resolve);
    worker.postMessage({ ...request, id });
  });
}

// ---------------------------------------------------------------- loading

/** Which file to load: ?puzzle=<name> for development, else today's date. */
function puzzleName() {
  const requested = new URLSearchParams(location.search).get("puzzle");
  if (requested && /^[A-Za-z0-9_-]+$/.test(requested)) {
    return requested;
  }
  return localDate();
}

async function loadJson(path) {
  const response = await fetch(path);
  if (!response.ok) {
    return null;
  }
  return { text: await response.text() };
}

// ---------------------------------------------------------------- the game

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function lineDetails(summary, line) {
  const details = el("details", "line");
  details.append(el("summary", null, summary), el("pre", null, line));
  return details;
}

async function main() {
  setZoomDialog($("zoom"));
  $("zoom").addEventListener("click", () => $("zoom").close());

  const name = puzzleName();
  const [file, imageFile] = await Promise.all([
    loadJson(`puzzles/${name}.json`),
    loadJson("card-images.json"),
  ]);
  setImages(imageFile ? JSON.parse(imageFile.text) : {});

  if (!file) {
    $("title").textContent = "No puzzle today";
    $("status").textContent = `There is no puzzle for ${name}. Check back tomorrow.`;
    return;
  }

  const puzzleText = file.text;
  const puzzle = JSON.parse(puzzleText);
  const id = puzzle.id ?? name;
  const allowed = puzzle.attempts ?? ATTEMPTS;
  const record = loadRecord(id);

  $("title").textContent = puzzle.title ?? "Doomdle";
  $("date").textContent = id;
  renderBoard($("board"), puzzle);

  const builder = createPileBuilder($("builder"), puzzle, submit);

  function solved() {
    return record.attempts.some((a) => a.result.wins);
  }

  function used() {
    return record.attempts.filter((a) => a.result.ok && !a.result.gave_up).length;
  }

  function renderHistory() {
    const list = $("history");
    list.replaceChildren();
    record.attempts.forEach((attempt, i) => {
      const item = el("li", attempt.result.wins ? "attempt won" : "attempt");
      item.append(el("p", "attempt-pile", `${i + 1}. ${attempt.pile.join(", ")}`));
      item.append(el("p", "attempt-verdict", verdictSentence(attempt.result)));
      if (attempt.result.wins && attempt.result.line) {
        item.append(lineDetails("Show the winning line", attempt.result.line));
      }
      const closest = attempt.result.feedback?.closest;
      if (closest?.line) {
        item.append(lineDetails("Show the closest line", closest.line));
      }
      list.append(item);
    });
  }

  async function renderStatus() {
    const left = allowed - used();
    if (solved()) {
      $("status").textContent = `Solved in ${used()} of ${allowed}.`;
      builder.setEnabled(false);
    } else if (left <= 0) {
      $("status").textContent = "Out of attempts.";
      builder.setEnabled(false);
      await reveal();
    } else {
      $("status").textContent = `${left} of ${allowed} attempts left.`;
      builder.setEnabled(true);
    }
  }

  async function reveal() {
    if (!REVEAL_ON_LOSS || $("reveal").childElementCount > 0) {
      return;
    }
    const answer = puzzle.solution
      ? await ask({ kind: "verify", puzzle: puzzleText, pile: puzzle.solution })
      : await ask({ kind: "solve", puzzle: puzzleText });
    const pile = puzzle.solution ?? answer.pile;
    if (!answer.ok || !pile) {
      return;
    }
    $("reveal").append(
      el("h3", null, "A winning pile"),
      el("p", null, pile.join(", ")),
      lineDetails("Show the line", answer.line ?? ""),
    );
  }

  async function submit(pile) {
    if (record.attempts.some((a) => a.pile.join("|") === pile.join("|") && !a.result.gave_up)) {
      $("status").textContent = "You already tried that pile.";
      return;
    }
    builder.setEnabled(false);
    $("status").textContent = "Checking…";
    const result = await ask({ kind: "verify", puzzle: puzzleText, pile });
    record.attempts.push({ pile, result });
    saveRecord(id, record);
    renderHistory();
    if (!result.wins) {
      builder.clear();
    }
    await renderStatus();
  }

  renderHistory();
  await renderStatus();
}

main().catch((error) => {
  $("title").textContent = "Doomdle";
  $("status").textContent = `The page failed to load: ${error}`;
});
