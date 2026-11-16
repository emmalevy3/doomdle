// The pile builder: pick up to five candidates and order them, top first.

import { cardElement } from "./board.js";

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

/**
 * A builder bound to `container`. `onSubmit(pile)` gets the card names, top
 * first. Call `setEnabled(false)` once the game is over.
 */
export function createPileBuilder(container, puzzle, onSubmit) {
  const pool = candidates(puzzle);
  const size = Math.min(5, pool.length);
  let picks = []; // indices into pool, top first
  let enabled = true;

  function render() {
    container.replaceChildren();

    const slots = el("ol", "slots");
    for (let i = 0; i < size; i++) {
      const slot = el("li", "slot");
      slot.append(el("span", "slot-label", i === 0 ? "1 (top)" : String(i + 1)));
      const index = picks[i];
      if (index === undefined) {
        slot.append(el("span", "slot-empty", "Empty"));
      } else {
        slot.append(cardElement(pool[index].name));
        const controls = el("span", "slot-controls");
        const button = (label, title, action, disabled) => {
          const b = el("button", "small", label);
          b.type = "button";
          b.title = title;
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

    const pickList = el("div", "cards candidates");
    pool.forEach((card, index) => {
      const used = picks.includes(index);
      const badges = card.from === "graveyard" ? ["graveyard"] : [];
      const node = cardElement(card.name, {
        badges,
        onClick: () => add(index),
      });
      node.disabled = used || !enabled || picks.length >= size;
      if (used) node.classList.add("used");
      pickList.append(node);
    });

    const submit = el("button", "primary", "Check this pile");
    submit.type = "button";
    submit.disabled = !enabled || picks.length !== size;
    submit.addEventListener("click", () => onSubmit(picks.map((i) => pool[i].name)));

    const clear = el("button", null, "Clear");
    clear.type = "button";
    clear.disabled = !enabled || picks.length === 0;
    clear.addEventListener("click", () => {
      picks = [];
      render();
    });

    const actions = el("div", "actions");
    actions.append(submit, clear);

    container.append(
      el("h3", null, `Your pile (top first, ${size} cards)`),
      slots,
      actions,
      el("h3", null, "Choose from library and graveyard"),
      pickList,
    );
  }

  function add(index) {
    if (picks.length < size && !picks.includes(index)) {
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
