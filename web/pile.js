// The pile builder: pick up to five candidates and order them, top first.

import { cardElement, largeImage } from "./board.js";

/**
 * The cards a pile can be built from: the library candidates, then the
 * graveyard, one entry per copy.
 */
export function candidates(puzzle) {
  const you = puzzle.you ?? {};
  return [
    ...(you.library ?? []).map((name) => ({ name, from: "library" })),
    ...(you.graveyard ?? []).map((name) => ({ name, from: "graveyard" })),
  ];
}

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

let peek = null;

/** Show `name` large beside `card` (a pick-stack card), without taking the pointer. */
function showPeek(name, card) {
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
  const width = peek.getBoundingClientRect().width || 300;
  const height = width * (88 / 63);
  const top = Math.min(Math.max(8, box.top), window.innerHeight - height - 8);
  peek.style.left = `${Math.max(8, Math.min(box.right + 16, window.innerWidth - width - 8))}px`;
  peek.style.top = `${Math.max(8, top)}px`;
}

/**
 * True on a touch screen (no hover) wide enough for the stacked layout, e.g.
 * a tablet held sideways: there the first tap on a candidate shows it and a
 * second tap picks it. Phones keep one tap to pick.
 */
function tapToPreview() {
  return window.matchMedia("(hover: none) and (min-width: 1000px)").matches;
}

function hidePeek() {
  if (peek) {
    peek.hidden = true;
  }
}

function slotLabel(i) {
  return i === 0 ? "1 · top" : String(i + 1);
}

/** A read-only row of slots showing `names`, top first (used to reveal a pile). */
export function pileRow(names) {
  const slots = el("ol", "slots");
  names.forEach((name, i) => {
    const slot = el("li", "slot filled");
    slot.append(el("span", "slot-label", slotLabel(i)), cardElement(name));
    slots.append(slot);
  });
  return slots;
}

/**
 * A builder drawn into two panels: `pool` (the cards to pick from, one
 * overlapping stack) and `pile` (the slots, top first, and the buttons).
 * `onSubmit(pile)` gets the card names, top first. Call `setEnabled(false)`
 * once the game is over.
 */
export function createPileBuilder({ pool: poolPanel, pile: pilePanel }, puzzle, onSubmit) {
  const pool = candidates(puzzle);
  const size = Math.min(5, pool.length);
  let picks = []; // indices into pool, top first
  let enabled = true;
  let previewed = null; // the candidate a first tap is showing (tapToPreview)

  // A tap anywhere outside the stack closes a tap preview.
  document.addEventListener("click", (event) => {
    if (previewed !== null && !poolPanel.contains(event.target)) {
      previewed = null;
      hidePeek();
    }
  });

  /**
   * The candidates as one stack. They stay real buttons that are never
   * `disabled`, so hovering still lifts a used card for reading; `add` refuses
   * what can't be picked.
   */
  function renderPool() {
    const stack = el("div", "pick-stack");
    pool.forEach((card, index) => {
      const used = picks.includes(index);
      const node = cardElement(card.name, { onClick: () => pick(index, node) });
      const pickable = enabled && !used && picks.length < size;
      node.setAttribute("aria-disabled", String(!pickable));
      node.title = used ? `${card.name} (in your pile)` : card.name;
      if (used) node.classList.add("used");
      node.addEventListener("mouseenter", () => showPeek(card.name, node));
      node.addEventListener("focus", () => showPeek(card.name, node));
      node.addEventListener("mouseleave", hidePeek);
      node.addEventListener("blur", hidePeek);
      stack.append(node);
    });
    const hint = tapToPreview() ? [el("p", "hint", "Tap to read, tap again to add.")] : [];
    poolPanel.replaceChildren(el("h2", null, "Cards to pick from"), ...hint, stack);
  }

  function render() {
    hidePeek();
    previewed = null;
    renderPool();

    const slots = el("ol", "slots");
    for (let i = 0; i < size; i++) {
      const index = picks[i];
      const slot = el("li", index === undefined ? "slot" : "slot filled");
      slot.append(el("span", "slot-label", slotLabel(i)));
      if (index === undefined) {
        slot.append(el("span", "slot-empty", "—"));
      } else {
        slot.append(cardElement(pool[index].name));
        const controls = el("span", "slot-controls");
        const button = (label, title, action, disabled) => {
          const b = el("button", "small", label);
          b.type = "button";
          b.title = title;
          b.setAttribute("aria-label", title);
          b.disabled = disabled || !enabled;
          b.addEventListener("click", action);
          controls.append(b);
        };
        button("↑", "Move up", () => swap(i, i - 1), i === 0);
        button("↓", "Move down", () => swap(i, i + 1), i === picks.length - 1);
        button("✕", "Remove", () => remove(i), false);
        slot.append(controls);
      }
      slots.append(slot);
    }

    const submit = el("button", "primary", "Submit pile");
    submit.type = "button";
    submit.disabled = !enabled || picks.length !== size;
    submit.addEventListener("click", () => onSubmit(picks.map((i) => pool[i].name)));

    const clear = el("button", "quiet", "Clear");
    clear.type = "button";
    clear.disabled = !enabled || picks.length === 0;
    clear.addEventListener("click", () => {
      picks = [];
      render();
    });

    const actions = el("div", "actions");
    actions.append(submit, clear);

    pilePanel.replaceChildren(
      el("h2", null, "Doomsday pile"),
      el("p", "hint", `Choose ${size} card${size === 1 ? "" : "s"} and order them, top first.`),
      slots,
      actions,
    );
  }

  /** A click on a candidate: on a tablet, show it first; otherwise pick it. */
  function pick(index, node) {
    if (tapToPreview() && previewed !== index) {
      previewed = index;
      showPeek(pool[index].name, node);
      return;
    }
    add(index);
  }

  function add(index) {
    if (enabled && picks.length < size && !picks.includes(index)) {
      picks.push(index);
      render();
    }
  }

  function remove(i) {
    picks.splice(i, 1);
    render();
  }

  function swap(a, b) {
    [picks[a], picks[b]] = [picks[b], picks[a]];
    render();
  }

  render();
  return {
    setEnabled(value) {
      enabled = value;
      render();
    },
    clear() {
      picks = [];
      render();
    },
  };
}
