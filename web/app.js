// Loads today's puzzle, runs the attempts, and shows the feedback.

import { ATTEMPTS, FIRST_PUZZLE, REVEAL_ON_LOSS } from "./config.js";
import { hidePeek, renderBoard, setImages, setZoomDialog, showLarge, showPeek } from "./board.js";
import {
  archiveDays,
  discordText,
  guestLine,
  localDate,
  recordStatus,
  shareText,
  shortDate,
  verdictSentence,
} from "./describe.js";
import { candidates, createPileBuilder, pileRow } from "./pile.js";
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

/** "medium" as "Medium". */
function tierName(difficulty) {
  return difficulty.charAt(0).toUpperCase() + difficulty.slice(1);
}

// ---------------------------------------------------------------- the archive

/**
 * The 🗓 button: a dialog listing every day from the first puzzle to today,
 * newest first, with each day's difficulty and this browser's result. The
 * files are fetched the first time it opens, for their difficulty and
 * attempts; a day whose file is missing is left out.
 */
function setUpArchive(current) {
  const dialog = $("archive");
  const list = $("archive-list");
  let filled = false;
  $("archive-open").addEventListener("click", () => {
    if (!filled) {
      filled = true;
      fillArchive(list, current);
    }
    dialog.showModal();
    list.querySelector(".current")?.scrollIntoView({ block: "nearest" });
  });
  $("archive-close").addEventListener("click", () => dialog.close());
  // A click outside the panel lands on the dialog itself, i.e. the backdrop.
  dialog.addEventListener("click", (event) => {
    if (event.target === dialog) dialog.close();
  });
}

function fillArchive(list, current) {
  const today = localDate();
  const days = archiveDays(FIRST_PUZZLE, today);
  if (days.length === 0) {
    list.append(el("li", "archive-empty", "No puzzles yet."));
    return;
  }
  for (const { date, number } of days) {
    const item = el("li");
    const row = el("a", "archive-row");
    row.href = date === today ? "./" : `?puzzle=${date}`;
    if (date === current) {
      row.classList.add("current");
      row.setAttribute("aria-current", "page");
    }
    const tier = el("span", "archive-tier");
    const guest = el("span", "archive-guest");
    const result = el("span");
    row.append(
      el("span", "archive-number", `#${number}`),
      el("span", "archive-date", shortDate(date)),
      tier,
      guest,
      result,
    );
    item.append(row);
    list.append(item);
    showResult(result, date, ATTEMPTS);
    loadJson(`puzzles/${date}.json`)
      .then((file) => {
        if (!file) {
          item.remove();
          return;
        }
        const puzzle = JSON.parse(file.text);
        if (puzzle.difficulty) {
          tier.className = `archive-tier badge-difficulty ${puzzle.difficulty}`;
          tier.textContent = tierName(puzzle.difficulty);
        }
        if (puzzle.guest) {
          guest.textContent = guestLine(puzzle.guest);
        }
        showResult(result, puzzle.id ?? date, puzzle.attempts ?? ATTEMPTS);
      })
      .catch(() => item.remove());
  }
}

/** This browser's result on a puzzle, e.g. "✓ Solved 2/3". */
function showResult(node, id, allowed) {
  const { kind, used } = recordStatus(loadRecord(id), allowed);
  node.className = `archive-result ${kind}`;
  node.textContent = {
    solved: `✓ Solved ${used}/${allowed}`,
    missed: "✗ Missed",
    playing: `In progress ${used}/${allowed}`,
    unplayed: "Not played",
  }[kind];
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

/**
 * Makes `node` show the card `name`: large beside it while a mouse hovers or
 * it has focus, and in the zoom dialog when clicked or tapped.
 */
function setUpCardReference(node, name) {
  if (!node) {
    return;
  }
  const hover = () => window.matchMedia("(hover: hover)").matches;
  const peek = () => {
    if (hover()) {
      showPeek(name, node);
    }
  };
  node.addEventListener("mouseenter", peek);
  node.addEventListener("focus", peek);
  node.addEventListener("mouseleave", hidePeek);
  node.addEventListener("blur", hidePeek);
  node.addEventListener("click", () => {
    hidePeek();
    showLarge(name);
  });
}

async function main() {
  setUpLayoutToggle();
  setZoomDialog($("zoom"));
  $("zoom").addEventListener("click", () => $("zoom").close());

  const name = puzzleName();
  setUpArchive(name);
  const [file, imageFile] = await Promise.all([
    loadJson(`puzzles/${name}.json`),
    loadJson("card-images.json"),
  ]);
  setImages(imageFile ? JSON.parse(imageFile.text) : {});
  setUpCardReference($("doomsday-ref"), "Doomsday");

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
  if (puzzle.difficulty) {
    $("date").append(
      el("span", `badge-difficulty ${puzzle.difficulty}`, tierName(puzzle.difficulty)),
    );
  }
  if (puzzle.guest) {
    $("date").append(el("span", "guest-credit", guestLine(puzzle.guest)));
  }
  renderBoard({ you: $("you"), hand: $("hand"), them: $("them") }, puzzle);

  const builder = createPileBuilder({ pool: $("pool"), pile: $("pile") }, puzzle, submit);
  // A solved puzzle reopens with its winning pile in place, as it was left.
  const win = record.attempts.find((a) => a.result.wins);
  if (win) {
    builder.setPile(win.pile);
  }

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

  /** Copy buttons for the finished game: plain text, and Discord with a spoiler. */
  function renderShare(finished) {
    const share = $("share");
    share.replaceChildren();
    if (!finished) {
      return;
    }
    const details = {
      id,
      attempts: record.attempts,
      allowed,
      difficulty: puzzle.difficulty,
      guest: puzzle.guest,
      pileSize: Math.min(5, candidates(puzzle).length),
      link: `${location.origin}${location.pathname}`,
    };
    const fallback = el("textarea", "share-text");
    fallback.readOnly = true;
    fallback.hidden = true;
    const button = (label, text) => {
      const b = el("button", null, label);
      b.type = "button";
      b.addEventListener("click", async () => {
        try {
          await navigator.clipboard.writeText(text);
          b.textContent = "Copied!";
          setTimeout(() => (b.textContent = label), 2000);
        } catch {
          // No clipboard access (needs HTTPS or localhost): select it instead.
          fallback.value = text;
          fallback.hidden = false;
          fallback.rows = text.split("\n").length;
          fallback.focus();
          fallback.select();
        }
      });
      return b;
    };
    const row = el("div", "share-buttons");
    row.append(
      button("Copy result", shareText(details)),
      button("Copy for Discord", discordText(details)),
    );
    share.append(row, fallback);
  }

  async function renderStatus() {
    const left = allowed - used();
    renderMarks();
    renderShare(solved() || left <= 0);
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
