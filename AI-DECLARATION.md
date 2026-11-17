---
version: "0.1.2"
level: copilot
processes:
  design: pair
  implementation: copilot
  testing: copilot
  documentation: copilot
  deployment: copilot
---

# AI declaration

Doomdle was built with Claude Code (Anthropic's coding assistant), following the [AI-DECLARATION.md 0.1.2](https://ai-declaration.md/en/0.1.2/) format.

## Notes

- **Design (pair):** I decided what the game is and how it plays. I wrote the card pool and how every card behaves (`CARDS.md`), the scope, the page layout, and the difficulty tiers, which came from my own play-testing. Claude proposed the architecture and a plan for each stage, and I approved or changed every plan before any work started.
- **Implementation, testing, documentation, deployment (copilot):** Claude wrote the Rust engine and solver, the tests, the website, the CI workflow and most of the project notes, asking before each change. I directed the work, reviewed it, and tested the game by playing it.
- **Puzzles involve no AI.** Each daily puzzle is dealt at random by the project's own generator. A deterministic solver then checks every possible pile to prove the puzzle has a solution and to rate its difficulty. No language model writes, picks or solves puzzles, and the site never sends anything to an AI service.
- Card images come from Scryfall. Magic: The Gathering is © Wizards of the Coast.
