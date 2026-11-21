// Draws cards and the puzzle's game state.

import { lifeAfterDoomsday } from "./describe.js";

let images = {};
let zoom = null;

/** The name to Scryfall image map, loaded once by app.js. */
export function setImages(map) {
  images = map ?? {};
}

/** The largest image URL for `name`, or undefined if there is none. */
export function largeImage(name) {
  return images[name]?.large ?? images[name]?.normal;
}

/** The dialog that shows a card large. */
export function setZoomDialog(dialog) {
  zoom = dialog;
}

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) {
    node.className = className;
  }
  if (text !== undefined) {
    node.textContent = text;
  }
  return node;
}

/**
 * One card: its Scryfall image, or a text tile if there is none. `badges` are
 * short labels shown on the card (loyalty, Cavern type, ...).
 */
export function cardElement(name, { badges = [], onClick } = {}) {
  const card = el("button", "card");
  card.type = "button";
  card.title = name;
  const image = images[name];
  if (image?.normal) {
    const img = el("img");
    img.src = image.normal;
    img.alt = name;
    img.loading = "lazy";
    card.append(img);
  } else {
    card.classList.add("text-card");
    card.append(el("span", "card-name", name));
  }
  if (badges.length > 0) {
    const row = el("span", "badges");
    for (const badge of badges) {
      row.append(el("span", "badge", badge));
    }
    card.append(row);
  }
  card.addEventListener("click", onClick ?? (() => showLarge(name)));
  return card;
}

/** Opens the dialog that shows `name` large (or its name, without an image). */
export function showLarge(name) {
  if (!zoom) {
    return;
  }
  const body = zoom.querySelector(".zoom-body");
  body.replaceChildren();
  const image = images[name];
  if (image?.large || image?.normal) {
    const img = el("img");
    img.src = image.large ?? image.normal;
    img.alt = name;
    body.append(img);
  } else {
    body.append(el("p", "zoom-name", name));
  }
  zoom.showModal();
}

let peek = null;

/**
 * Shows `name` large beside `card` without taking the pointer: level with
 * `card`, to the right of `beside` (the card itself unless given, e.g. the
 * whole pick stack), or to its left when there is no room.
 */
export function showPeek(name, card, beside = card) {
  const url = largeImage(name);
  if (!url) {
    return;
  }
  if (!peek) {
    peek = el("img", "peek");
    peek.alt = "";
    document.body.append(peek);
  }
  peek.src = url;
  peek.hidden = false;
  const box = card.getBoundingClientRect();
  const side = beside.getBoundingClientRect();
  const width = peek.getBoundingClientRect().width || 300;
  const height = width * (88 / 63);
  const top = Math.min(Math.max(8, box.top), window.innerHeight - height - 8);
  const right = side.right + 16;
  const left = right + width + 8 <= window.innerWidth ? right : side.left - width - 16;
  peek.style.left = `${Math.max(8, Math.min(left, window.innerWidth - width - 8))}px`;
  peek.style.top = `${Math.max(8, top)}px`;
}

export function hidePeek() {
  if (peek) {
    peek.hidden = true;
  }
}

/** True where a mouse can hover: there your permanents can be tapped to plan. */
function canPlan() {
  return window.matchMedia("(hover: hover)").matches;
}

/** A battlefield entry from the puzzle file: a name or { card, ... }. */
function permanent(entry) {
  if (typeof entry === "string") {
    return { card: entry };
  }
  return entry;
}

function permanentBadges(p) {
  const badges = [];
  if (p.summoning_sick) badges.push("summoning sick");
  if (p.loyalty !== undefined) badges.push(`loyalty ${p.loyalty}`);
  if (p.type) badges.push(p.type);
  if (p.greaves) badges.push("Greaves");
  if (p.skateboard) badges.push("Skateboard");
  return badges;
}

function zone(label, entries, toCard) {
  const section = el("section", "zone");
  section.append(el("h3", null, label), cardRow(entries, toCard));
  return section;
}

/** A battlefield card, turned sideways in a square frame when tapped. */
function permanentCard(p) {
  const card = cardElement(p.card, { badges: permanentBadges(p) });
  if (!p.tapped) {
    return card;
  }
  card.classList.add("tapped");
  card.title = `${p.card} (tapped)`;
  const frame = el("span", "tapped-frame");
  frame.append(card);
  return frame;
}

/**
 * Your battlefield. With a mouse, clicking an untapped permanent taps it in
 * your plan (and again untaps it), just to help picture a line; it changes
 * nothing in the puzzle. Hovering shows the card large instead of a click.
 * Permanents the puzzle has tapped stay tapped.
 */
function yourBattlefield(entries) {
  const section = el("section", "zone");
  const head = el("div", "zone-head");
  head.append(el("h3", null, "Battlefield"));
  const untapAll = el("button", "quiet small untap-all", "Untap all");
  untapAll.type = "button";
  untapAll.hidden = true;
  head.append(untapAll);
  section.append(head);
  if (!canPlan()) {
    section.append(cardRow(entries, permanentCard));
    return section;
  }

  const untaps = [];
  let planned = 0;
  const changed = (delta) => {
    planned += delta;
    untapAll.hidden = planned === 0;
  };
  untapAll.addEventListener("click", () => {
    for (const untap of untaps) {
      untap();
    }
  });

  const plannable = (p) => {
    if (p.tapped) {
      return permanentCard(p);
    }
    const holder = el("span", "perm");
    let tapped = false;
    const card = cardElement(p.card, {
      badges: permanentBadges(p),
      onClick: () => set(!tapped),
    });
    const set = (value) => {
      if (value === tapped) {
        return;
      }
      tapped = value;
      card.classList.toggle("tapped", tapped);
      card.classList.toggle("planned", tapped);
      card.title = tapped ? `${p.card} (tapped in your plan; click to untap)` : `${p.card} (click to tap)`;
      if (tapped) {
        const frame = el("span", "tapped-frame");
        frame.append(card);
        holder.replaceChildren(frame);
      } else {
        holder.replaceChildren(card);
      }
      changed(tapped ? 1 : -1);
    };
    card.title = `${p.card} (click to tap)`;
    card.addEventListener("mouseenter", () => showPeek(p.card, card));
    card.addEventListener("mouseleave", hidePeek);
    card.addEventListener("click", hidePeek);
    holder.append(card);
    untaps.push(() => set(false));
    return holder;
  };
  section.append(
    el("p", "hint plan-hint", "Click your permanents to tap them while you plan."),
    cardRow(entries, plannable),
  );
  return section;
}

/** Small labelled values: [label, value] pairs, or [label, value, extra class]. A label with no value stands alone. */
function stats(items) {
  const list = el("ul", "stats");
  for (const [label, value, extra] of items) {
    const item = el("li", extra ? `stat ${extra}` : "stat");
    item.append(el("span", "stat-label", label));
    if (value !== undefined) item.append(el("span", "stat-value", value));
    list.append(item);
  }
  return list;
}

/** A panel: a heading, then whatever `children` are. */
function panel(target, heading, ...children) {
  target.replaceChildren(el("h2", null, heading), ...children);
}

function cardRow(entries, toCard) {
  const row = el("div", "cards");
  if (entries.length === 0) {
    row.append(el("p", "empty", "Nothing"));
  }
  for (const entry of entries) {
    row.append(toCard(entry));
  }
  return row;
}

/**
 * The game state from the parsed puzzle file, drawn into three panels:
 * `you` (your stats and battlefield), `hand` (your hand) and `them` (the
 * opponent's stats, revealed hand and battlefield).
 */
export function renderBoard({ you: yourPanel, hand: handPanel, them: theirPanel }, puzzle) {
  const you = puzzle.you ?? {};
  const them = puzzle.opponent ?? {};
  const life = you.life ?? 20;

  const yourStats = [
    [you.land_drop === false ? "Land drop was used" : "Land drop is available"],
  ];
  if (you.mana_pool) yourStats.unshift(["Floating mana", you.mana_pool, "alert"]);
  if (you.spells_cast) yourStats.push(["Spells cast", String(you.spells_cast)]);
  if (you.cards_drawn) yourStats.push(["Cards drawn", String(you.cards_drawn)]);
  if (you.yawgmoths_will) yourStats.push(["Yawgmoth's Will", "active"]);
  // Life after Doomsday is what you play with, so it is the big number.
  const lifeBlock = el("p", "life");
  lifeBlock.append(
    el("span", "life-value", String(lifeAfterDoomsday(life))),
    el("span", "life-label", "life"),
    el("span", "life-before", `(${life} before Doomsday)`),
  );
  panel(
    yourPanel,
    "Your board",
    lifeBlock,
    stats(yourStats),
    yourBattlefield((you.battlefield ?? []).map(permanent)),
  );

  panel(handPanel, "Your hand", cardRow(you.hand ?? [], (name) => cardElement(name)));

  panel(
    theirPanel,
    "Opponent",
    stats([["Life", String(them.life ?? 20)]]),
    zone("Hand (revealed)", them.hand ?? [], (name) => cardElement(name)),
    zone("Battlefield", (them.battlefield ?? []).map(permanent), permanentCard),
  );
}
