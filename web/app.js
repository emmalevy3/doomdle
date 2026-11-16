// Loads today's puzzle, runs the attempts, and shows the feedback.

import { ATTEMPTS, REVEAL_ON_LOSS } from "./config.js";
import { renderBoard, setImages, setZoomDialog } from "./board.js";
import { verdictSentence, localDate } from "./describe.js";
import { createPileBuilder, pileRow } from "./pile.js";
import { loadRecord, loadSetting, saveRecord, saveSetting } from "./storage.js";

const $ = (id) => document.getElementById(id);

// ---------------------------------------------------------------- verifier

const waiting = new Map();
let nextId = 0;
let worker = null;

/** The verifier worker, started on first use. */
function verifier() {
  if (worker) {
    return worker;
  }
  worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
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
  return worker;
}

function ask(request) {
  const worker = verifier();
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

/**
 * Which side sits on top: "you" (the default) or "opponent" (like a real
 * table). Remembered in this browser; the CSS reads the body class.
 */
function setUpLayoutToggle() {
  const button = $("layout-toggle");
  button.setAttribute("aria-label", "Swap which side is on top");
  const apply = (order) => {
    const opponentTop = order === "opponent";
    document.body.classList.toggle("opponent-top", opponentTop);
    button.setAttribute("aria-pressed", String(opponentTop));
    button.textContent = opponentTop ? "⇅ Put you on top" : "⇅ Put the opponent on top";
  };
  apply(loadSetting("order", "you"));
  button.addEventListener("click", () => {
    const order = document.body.classList.contains("opponent-top") ? "you" : "opponent";
    saveSetting("order", order);
    apply(order);
  });
}

async function main() {
  setUpLayoutToggle();
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
  renderBoard({ you: $("you"), hand: $("hand"), them: $("them") }, puzzle);

  const builder = createPileBuilder({ pool: $("pool"), pile: $("pile") }, puzzle, submit);

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

  /** One mark per allowed attempt: ✦ won, ✕ failed, ○ unused. */
  function renderMarks() {
    const counted = record.attempts.filter((a) => a.result.ok && !a.result.gave_up);
    const marks = $("marks");
    marks.replaceChildren();
    for (let i = 0; i < allowed; i++) {
      const attempt = counted[i];
      const [symbol, kind] = !attempt ? ["○", "unused"] : attempt.result.wins ? ["✦", "won"] : ["✕", "failed"];
      marks.append(el("span", `mark ${kind}`, symbol));
    }
  }

  async function renderStatus() {
    const left = allowed - used();
    renderMarks();
    $("pile-area").classList.toggle("complete", solved());
    if (solved()) {
      $("status").textContent = `You win — solved in ${used()} of ${allowed}.`;
      builder.setEnabled(false);
    } else if (left <= 0) {
      $("status").textContent = "Out of attempts.";
      builder.setEnabled(false);
      await reveal();
    } else {
      $("status").textContent = `${left} of ${allowed} attempt${left === 1 ? "" : "s"} left.`;
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
      pileRow(pile),
      lineDetails("Show the line", answer.line ?? ""),
    );
  }

  async function submit(pile) {
    if (record.attempts.some((a) => a.pile.join("|") === pile.join("|") && !a.result.gave_up)) {
      $("status").textContent = "You already tried that pile.";
      return;
    }
    builder.setEnabled(false);
    $("status").textContent = "Resolving the pile…";
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

/**
 * ?view=phone (for development): show the page inside a phone-sized frame, so
 * the phone layout's media queries apply. The frame loads the same address
 * without `view`. Returns whether it took over the page.
 */
function phoneView() {
  const params = new URLSearchParams(location.search);
  if (params.get("view") !== "phone" || window.self !== window.top) {
    return false;
  }
  params.delete("view");
  const frame = el("iframe", "phone-frame");
  frame.title = "Doomdle at phone size";
  frame.src = `${location.pathname}?${params}`;
  const view = el("div", "phone-view");
  view.append(el("p", "phone-label", "Phone preview · 390 × 844"), frame);
  document.body.replaceChildren(view);
  return true;
}

if (!phoneView()) {
  main().catch((error) => {
    $("title").textContent = "Doomdle";
    $("status").textContent = `The page failed to load: ${error}`;
  });
}
