# Card pool

How each card works **in this engine**. Oracle text is current as of Scryfall (Nov 2026). Reviewed: treat every gotcha and simplification here as authoritative. If something isn't covered, stop and ask.

Per card:
- **Cost / type:** mana cost and card type, plus any alternative costs
- **Effect:** current Oracle text
- **Gotchas:** rules details that matter for piles and are easy to get wrong
- **Simplifications:** what the engine ignores or restricts (proposed)

---

## Engine-wide notes

### State this pool requires
Beyond hand / library / graveyard / mana / life, these cards need the engine to track:
- **Land drop available** (Undercity Sewers, Meticulous Archive, lands from Reckless Impulse)
- **Storm count**: spells cast this turn (Flusterstorm)
- **Whose turn it is** (Force of Negation's alternative cost, Defense Grid)
- **Spells the opponent cast this turn, and their colors** (Veil of Summer's draw)
- **Tapped status and summoning sickness** for permanents (Doctor Doom, Wizard's Rockets)
- **Haste and equipment attachments**: which creature each Equipment is attached to (Lightning Greaves, Skateboard)
- **Chosen creature type** for Cavern of Souls
- **Planeswalker loyalty and whether it has activated this turn** (Jace, Teferi)
- **An "exiled, may play" zone** (Reckless Impulse)
- **Whether the library order is still known** (see shuffles below)
- **Cards drawn this turn** (Tamiyo transforms on the third; Narset, Bowmasters)
- **Yawgmoth's Will active**: cards in the graveyard are castable/playable, and cards going to the graveyard are exiled instead
- **Controller of each permanent**: some hate is symmetric (Thalia, Archon's spell limit, Grafdigger's Cage) and some only hits opponents (Narset, Leyline of the Void, Archon's tapped lands)
- **Land subtypes**: Island, Swamp, etc. (fetchlands, Daze, Gush, Snuff Out)
- **The stack and priority**: spells and triggered abilities go on the stack and both players get priority before anything resolves. Needed for every response, for LED (activated with a spell on the stack), and for Yawgmoth's Will + LED.
- **Opponent state**: hand (open), untapped lands and other mana sources, life, graveyard, permanents, and spells cast this turn (Mindbreak Trap counts *your* spells; Archon limits theirs)
- **Creature toughness and damage** (Bowmasters pings, e.g. on Lab Man)

### Decisions
1. **Shuffles (decided).** Ponder never shuffles. After any other shuffle (fetchlands, casting Edge of Autumn) or random bottom-of-library placement (Thundertrap Trainer), the library is marked unordered, and any later step that depends on library order counts as a failure. Order-independent lines, such as drawing the last card or Oracle with enough devotion, still work. An effect that covers the whole remaining library (drawing every card left, Ponder on three or fewer cards, scry or surveil that sees all of it) also still works, and because it places every card it makes the library ordered again. Searching the library (fetchlands, Edge of Autumn) never depends on order, so it is always legal on an unordered library; a shuffle that leaves one or zero cards behind doesn't mark the library unordered, since the order is trivially known.
2. **Opponent interaction (decided).** The opponent responds but never acts proactively: whenever they get priority, they either pass or cast an instant or alternative-cost spell from their open hand (Force of Will, Daze, Stifle, etc.), and they make the choices on their own effects (Bowmasters' target, Daze's returned Island). A pile is valid only if there's a line that wins *against every opponent response*: an AND/OR search where any one of your choices may win, but every opponent choice must still lose. "Unless its controller pays" (Daze, Spell Pierce, Miscalculation, Flusterstorm) is your choice and a branch. Keep puzzles small: at most 3 cards in the opponent's hand and 3 opponent mana sources. Engine defaults (decided): the opponent casts only *targeted* spells aimed at your stack items and never floats mana, tapping lands as part of casting or of paying a tax; they never cycle or play lands. Nobody targets their own stack items. A targeted spell whose target has left the stack by the time it resolves does nothing and still goes to the graveyard. You pay an "unless its controller pays" tax from mana already in your pool, so tap in response while the counter is on the stack. The opponent at 0 life (Force of Will, Phyrexian mana) loses: you win.
3. **Board state at puzzle start.** Wizard's Rockets is only useful if it starts on the battlefield. Doctor Doom needs to start on the battlefield or get haste from Lightning Greaves or Skateboard. Cavern of Souls needs its creature type set in the puzzle file if it starts in play.
4. **Turn history at puzzle start.** The puzzle file sets how many spells you've cast this turn *before* Doomsday (the engine adds Doomsday itself when the pile is chosen) and how many cards you've drawn this turn (including the draw step). Archon of Emeria, Narset, Tamiyo, Flusterstorm, and Mindbreak Trap all depend on these.
5. **Puzzle start point (decided).** The puzzle starts with Doomsday resolving: the player picks 5 cards from library + graveyard and orders them. Doomsday has already been cast and paid for, and it can no longer be countered.
6. **Duplicates (decided).** A puzzle may contain more than one copy of a card (real lists play 2 Oracles). Zones hold `Card` values, so copies are just repeated entries.
7. **Edge of Autumn (decided).** Both modes are modeled: cycling, and casting it to fetch a basic out of the pile.
8. **Timetwister (decided).** Only allowed in puzzles where the opponent holds no interaction.

---

## Win conditions

### Thassa's Oracle
- **Cost / type:** {U}{U} — Creature — Merfolk Wizard
- **Effect:** When this creature enters, look at the top X cards of your library, where X is your devotion to blue. Put up to one of them on top of your library and the rest on the bottom of your library in a random order. If X is greater than or equal to the number of cards in your library, you win the game.
- **Gotchas:** Devotion is counted when the trigger *resolves*, and Oracle counts itself (2). In this pool, Jace adds 3, Riddler 2, Teferi 1, Lab Man 1, Tamiyo (front face) 1, and Thundertrap Trainer 1, plus 1 for its token copy. Removing a blue permanent in response lowers X (Swords, Pyroblast, Snuff Out). Moving cards to the bottom doesn't change the library size. An empty library always wins. The *trigger* can be countered by Stifle or Consign to Memory; Cavern only protects the creature spell, not its trigger.
- **Simplifications:** if the trigger doesn't win, the engine marks the library unordered (the rest go to the bottom in a random order) and skips the "put up to one of them on top" choice, since the scrambled library can't be exploited afterwards anyway.

### Jace, Wielder of Mysteries
- **Cost / type:** {1}{U}{U}{U} — Legendary Planeswalker — Jace (starting loyalty 4)
- **Effect:** If you would draw a card while your library has no cards in it, you win the game instead. +1: Target player mills two cards. Draw a card. −8: Draw seven cards. Then if your library has no cards in it, you win the game.
- **Gotchas:** The static replaces a draw from an empty library, so with Jace in play, any draw into an empty library wins. Targeting yourself with +1 mills 2 *then* draws 1, removing 3 cards from the pile. Loyalty abilities are sorcery speed, once per turn. Adds 3 devotion for Oracle. Cards exiled by Reckless Impulse are not draws.
- **Simplifications:** −8 is unreachable in one turn from loyalty 4, so the engine can omit it.

### Doctor Doom, Unrivaled
- **Cost / type:** {2}{B}{B} — Legendary Creature — Human Sorcerer Villain
- **Effect:** Lifelink. {T}: You draw a card and lose 1 life. Then if your library has no cards in it, you win the game. (You win even if you have 0 life or didn't draw a card.)
- **Gotchas:** The ability needs {T}, so summoning sickness applies. Doom works the turn he's cast only with haste from Lightning Greaves or Skateboard; otherwise he must start the puzzle on the battlefield. Each activation draws one card, and he can only tap once, so the library must have at most one card left when you activate. He wins even at 0 life. Not blue, so no devotion.
- **Simplifications:** ignore lifelink.

### Laboratory Maniac
- **Cost / type:** {2}{U} — Creature — Human Wizard
- **Effect:** If you would draw a card while your library has no cards in it, you win the game instead.
- **Gotchas:** Same replacement as Jace's static, on a 3-mana creature. That makes it a Wizard (Cavern), mana value 3 (Unearth can return it), and 1 devotion for Oracle. It's exposed to Swords and Snuff Out, unlike Oracle, which wins on entering.
- **Simplifications:**

---

## Haste

### Lightning Greaves
- **Cost / type:** {2} — Artifact — Equipment
- **Effect:** Equipped creature has haste and shroud. (It can't be the target of spells or abilities.) Equip {0}
- **Gotchas:** Equipping is sorcery speed but free, so total cost is {2}. Haste only has to be there when you activate Doom's {T} ability. Shroud blocks your own targeting too (e.g. Aether Spellbomb's bounce), and it stops the opponent's Swords and Snuff Out on Doom. Once Greaves is on the only creature, nothing can re-equip that creature.
- **Simplifications:** ignore combat; haste matters only for Doom's {T} ability.

### Skateboard
- **Cost / type:** {1} — Artifact — Equipment
- **Effect:** When this Equipment enters, tap target permanent. Equipped creature gets +1/+0 and has haste. Equip {1}
- **Gotchas:** Total cost is {2} ({1} to cast, {1} to equip, sorcery speed), the same as Greaves but split, and it doesn't give shroud. The enter trigger is mandatory and targets *any* permanent, so with nothing better, target a land you already tapped so you don't lose mana. It can also tap an opponent's land to deny counterspell mana.
- **Simplifications:** ignore the +1/+0.

---

## Doomsday

### Doomsday
- **Cost / type:** {B}{B}{B} — Sorcery
- **Effect:** Search your library and graveyard for five cards and exile the rest. Put the chosen cards on top of your library in any order. You lose half your life, rounded up.
- **Gotchas:** You must choose exactly five if five or more are available; with fewer, all of them go into the library. The graveyard is exiled too, except for chosen cards. Life loss rounds *up*: 20 → 10, 7 → 3. Doomsday itself goes to the graveyard *after* resolving, so the post-Doomsday graveyard starts with one sorcery in it. That counts for Flow State and Cabal Ritual's threshold. Under an opponent's Leyline of the Void it's exiled instead.
- **Simplifications:** the puzzle starts as this resolves (see CLAUDE.md). The puzzle's life total is the one *before* Doomsday's life loss; the engine applies the halving when the pile is chosen.

---

## Graveyard

### Unearth
- **Cost / type:** {B} — Sorcery. Cycling {2}.
- **Effect:** Return target creature card with mana value 3 or less from your graveyard to the battlefield. Cycling {2}
- **Gotchas:** Legal targets in this pool: Thassa's Oracle, Laboratory Maniac, Thundertrap Trainer, Tamiyo (front face, mana value 1). Not Doom, Grief, Street Wraith, or Quantum Riddler. Oracle returned this way still triggers on entering. Ways to get Oracle into the graveyard after Doomsday: LED or Frantic Search discards, surveil (Consider, Sewers, Archive), Jace +1 mill. Cycling costs {2} and can't be done from the graveyard under Yawgmoth's Will.
- **Simplifications:** the target is chosen on cast; with no legal target in the graveyard, Unearth can't be cast (cycling it is still allowed). If the target has left the graveyard by the time Unearth resolves, the spell does nothing and still goes to the graveyard.

### Yawgmoth's Will
- **Cost / type:** {2}{B} — Sorcery
- **Effect:** Until end of turn, you may play lands and cast spells from your graveyard. If a card would be put into your graveyard from anywhere this turn, exile that card instead.
- **Gotchas:** Order is everything. Cards that hit the graveyard *before* Will resolves are replayable. After it resolves, anything that would go to the graveyard, including discards and spells you recast, is exiled instead. Will exiles itself. Classic line: cast Will, hold priority, crack LED so your hand and LED go to the graveyard, then replay them from the graveyard. Lands from the graveyard use your land drop. Cycling and other "discard this card" abilities can't be used from the graveyard.
- **Simplifications:** Doomsday is never castable again, so Will doesn't offer a second pile choice from the graveyard. A puzzle may start with Will already active; Doomsday is then exiled when the pile is chosen instead of starting the graveyard.

---

## Draw and selection

### Flow State
- **Cost / type:** {1}{U} — Sorcery
- **Effect:** Look at the top three cards of your library. Put one of them into your hand and the rest on the bottom of your library in any order. If there is an instant card and a sorcery card in your graveyard, instead put two of them into your hand and the rest on the bottom of your library in any order.
- **Gotchas:** The check happens on resolution. Doomsday in the graveyard already covers the sorcery half, so you usually need just one instant there first, such as Consider, Brainstorm, or Dark Ritual. Artifacts like a cracked Lotus Petal don't count. The bottom order is chosen, so the library stays ordered (unlike Thundertrap Trainer).
- **Simplifications:**

### Gush
- **Cost / type:** {4}{U} — Instant. Alt: return two Islands you control to their owner's hand.
- **Effect:** You may return two Islands you control to their owner's hand rather than pay this spell's mana cost. Draw two cards.
- **Gotchas:** Tap the Islands for mana first, then return them as the cost, so it's free and the mana floats. One returned land can be replayed if a land drop remains (Sewers or Archive can surveil again). It competes with Daze for Islands, and the returned lands make your hand bigger (bad for Quantum Riddler). Mana value 5, so Mental Misstep can't hit it.
- **Simplifications:**

### Quantum Riddler
- **Cost / type:** {3}{U}{U} — Creature — Sphinx. Warp {1}{U}.
- **Effect:** Flying. When this creature enters, draw a card. As long as you have one or fewer cards in hand, if you would draw one or more cards, you draw that many cards plus one instead. Warp {1}{U}
- **Gotchas:** The bonus applies once per draw *instruction*: Brainstorm draws 4, Probe 2, Timetwister 8. The hand size is checked at the moment of each draw, and the spell being cast isn't in your hand. Its own enter-draw gets the bonus too if your hand is small. Devotion 2 from its printed cost even when warped. Mana value 5, so Unearth can't return it.
- **Simplifications:** ignore warp's end-step exile.

### Tamiyo, Inquisitive Student
- **Cost / type:** {U} — Legendary Creature — Moonfolk Wizard (transforms into Tamiyo, Seasoned Scholar, Legendary Planeswalker, loyalty 2)
- **Effect:** Flying. Whenever Tamiyo attacks, investigate. When you draw your third card in a turn, exile Tamiyo, then return her to the battlefield transformed. // Seasoned Scholar: +2: attackers get −1/−0. −3: Return target instant or sorcery card from your graveyard to your hand; if it's a green card, add one mana of any color. −7: Draw cards equal to half your library, rounded up; no maximum hand size.
- **Gotchas:** Counts *all* draws this turn, including ones before she entered. The transform is mandatory. The back face starts at loyalty 2, so −3 can't be used the turn she flips. The back face has no mana cost, so flipping *drops* her devotion from 1 to 0, which can break an Oracle line. Draw count becomes something to manage.
- **Simplifications:** ignore attacking and the +2 and −7 abilities.

### Manamorphose
- **Cost / type:** {1}{R/G} — Instant
- **Effect:** Add two mana in any combination of colors. Draw a card.
- **Gotchas:** Net free, and it fixes colors. You choose the colors before drawing. Counts as an instant for Flow State and as a spell for storm. Mana value 2.
- **Simplifications:**

### Frantic Search
- **Cost / type:** {2}{U} — Instant
- **Effect:** Draw two cards, then discard two cards. Untap up to three lands.
- **Gotchas:** Free if you have three tapped lands. The draw and discard happen together, so you can draw Oracle and discard it for Unearth, or discard an instant and sorcery for Flow State. The discard can also fuel Cabal Ritual's threshold.
- **Simplifications:** the untap always untaps as many lands as it can (up to three of your tapped lands); untapping fewer is never offered.

### Meditate
- **Cost / type:** {2}{U} — Instant
- **Effect:** Draw four cards. You skip your next turn.
- **Gotchas:** The skipped turn never matters in a win-this-turn puzzle.
- **Simplifications:** ignore the skipped turn.

### Timetwister
- **Cost / type:** {2}{U} — Sorcery
- **Effect:** Each player shuffles their hand and graveyard into their library, then draws seven cards.
- **Gotchas:** Timetwister itself isn't shuffled in. The library becomes unordered, but if it has exactly 7 cards you draw it all and can win with Oracle. If it has more, the rest stays unordered. If it has fewer, you'd draw from an empty library and lose, unless Lab Man or Jace turns the extra draw into a win. Your hand is shuffled in first, so with Quantum Riddler you draw 8.
- **Simplifications:** only allowed in puzzles where the opponent holds no interaction, since it gives them a new, unknown hand and breaks open information. In those puzzles, ignore the opponent's shuffle.

### Brainstorm
- **Cost / type:** {U} — Instant
- **Effect:** Draw three cards, then put two cards from your hand on top of your library in any order.
- **Gotchas:** You can put back *any* two cards from hand, not just ones you drew. That lets you re-stack the pile or store a card. Net effect on library size is −1. Mental Misstep target (mana value 1).
- **Simplifications:**

### Ponder
- **Cost / type:** {U} — Sorcery
- **Effect:** Look at the top three cards of your library, then put them back in any order. You may shuffle. Draw a card.
- **Gotchas:** Reordering the top three can fix a pile built in the wrong order.
- **Simplifications:** never shuffle.

### Preordain
- **Cost / type:** {U} — Sorcery
- **Effect:** Scry 2, then draw a card. (Look at the top two cards, put any number on the bottom and the rest on top in any order.)
- **Gotchas:** Scrying cards to the bottom doesn't shrink the library, so it doesn't help Oracle. It does change which card you draw next.
- **Simplifications:**

### Consider
- **Cost / type:** {U} — Instant
- **Effect:** Surveil 1, then draw a card. (Look at the top card; you may put it into your graveyard.)
- **Gotchas:** Can remove up to 2 cards from the pile for one mana. Instant, so it works with LED mana floating.
- **Simplifications:**

### Ancestral Recall
- **Cost / type:** {U} — Instant
- **Effect:** Target player draws three cards.
- **Gotchas:** Instant speed.
- **Simplifications:** always targets yourself.

### Night's Whisper
- **Cost / type:** {1}{B} — Sorcery
- **Effect:** You draw two cards and lose 2 life.
- **Gotchas:** Life loss can kill you after Doomsday halves your life. Losing happens at the next state-based check, so a win during the same resolution still counts.
- **Simplifications:**

### Ideas Unbound
- **Cost / type:** {U}{U} — Sorcery — Arcane
- **Effect:** Draw three cards. Discard three cards at the beginning of the next end step.
- **Gotchas:** Adds no devotion; it's a sorcery, not a permanent.
- **Simplifications:** ignore the end-step discard, since a winning line ends before then.

### Reckless Impulse
- **Cost / type:** {1}{R} — Sorcery
- **Effect:** Exile the top two cards of your library. Until the end of your next turn, you may play those cards.
- **Gotchas:** Shrinks the library by 2 without *drawing*, so it doesn't trigger Jace's replacement. You still pay costs and follow timing for exiled cards. A land among them uses your land drop.
- **Simplifications:**

### Gitaxian Probe
- **Cost / type:** {U/P} — Sorcery (pay {U} or 2 life)
- **Effect:** Look at target player's hand. Draw a card.
- **Gotchas:** Free with 2 life, which is a common way to open a pile. You can only pay life if your life total is at least 2; paying down to 0 loses at the next state-based check. Mana value is 1 (Mental Misstep).
- **Simplifications:** ignore the peek, since information is already open.

### Street Wraith
- **Cost / type:** {3}{B}{B} — Creature — Wraith. Cycling—Pay 2 life.
- **Effect:** Swampwalk. Cycling—Pay 2 life. (Pay 2 life, discard this card: draw a card.)
- **Gotchas:** Cycling costs no mana and isn't casting a spell. That means it can't be countered and doesn't add to storm. It uses the same life-payment rule as Probe.
- **Simplifications:** only the cycling mode is modeled.

### Edge of Autumn
- **Cost / type:** {1}{G} — Sorcery. Cycling—Sacrifice a land.
- **Effect:** If you control four or fewer lands, search your library for a basic land card, put it onto the battlefield tapped, then shuffle. Cycling—Sacrifice a land. (Sacrifice a land, discard this card: draw a card.)
- **Gotchas:** Cycling costs no mana or life, just a land, and you can sacrifice a land you've already tapped for mana. Casting it pulls a basic out of the pile (library −1), but the land enters tapped, so no mana this turn, and the shuffle makes the library unordered. It checks your land count on resolution; with five or more lands it does nothing.
- **Simplifications:** none; both modes are modeled.

### Thundertrap Trainer
- **Cost / type:** {1}{U} — Creature — Otter Wizard. Offspring {4}.
- **Effect:** Offspring {4} (You may pay an additional {4} as you cast this spell. If you do, when this creature enters, create a 1/1 token copy of it.) When this creature enters, look at the top four cards of your library. You may reveal a noncreature, nonland card from among them and put it into your hand. Put the rest on the bottom of your library in a random order.
- **Gotchas:** The cards that go to the bottom are in a *random* order, which scrambles the pile unless the order no longer matters. It can only take a noncreature, nonland card, so it can't find Oracle or Doom. The offspring token also has the trigger and adds 1 devotion. It's a Wizard, so Cavern set to Wizard makes it uncounterable.
- **Simplifications:** treat the bottom-of-library cards as making the library unordered (same rule as shuffles).

### Aether Spellbomb
- **Cost / type:** {1} — Artifact
- **Effect:** {U}, Sacrifice this artifact: Return target creature to its owner's hand. {1}, Sacrifice this artifact: Draw a card.
- **Gotchas:** Costs 2 total to draw a card. Mental Misstep target.
- **Simplifications:** model both modes; the bounce can save a creature from the opponent's response or reset an enter trigger.

---

## Mana

### Dark Ritual
- **Cost / type:** {B} — Instant
- **Effect:** Add {B}{B}{B}.
- **Gotchas:** It's a spell, not a mana ability, so it can be countered and it counts toward storm. Mental Misstep target.
- **Simplifications:**

### Lion's Eye Diamond
- **Cost / type:** {0} — Artifact
- **Effect:** Discard your hand, Sacrifice this artifact: Add three mana of any one color. Activate only as an instant.
- **Gotchas:** It's a mana ability that can only be activated when you could cast an instant. That means you need priority, so it can *never* be activated while casting a spell. The standard Doomsday use: cast a draw spell like Probe or Brainstorm, and with it on the stack, crack LED (discarding your hand). The draw spell resolves, you draw from the pile, and you cast with the floating mana. Mana stays in the pool until the step or phase ends, not when a spell resolves.
- **Simplifications:** mana pool empties only at end of phase; with one main phase, it effectively never empties mid-puzzle.

### Black Lotus
- **Cost / type:** {0} — Artifact
- **Effect:** {T}, Sacrifice this artifact: Add three mana of any one color.
- **Gotchas:** A normal mana ability with no timing restriction.
- **Simplifications:**

### Lotus Petal
- **Cost / type:** {0} — Artifact
- **Effect:** {T}, Sacrifice this artifact: Add one mana of any color.
- **Gotchas:** none.
- **Simplifications:**

### Gleemox
- **Cost / type:** {0} — Artifact
- **Effect:** {T}: Add one mana of any color. (Also says "This card is banned.")
- **Gotchas:** Reusable each turn, but only taps once in a one-turn puzzle.
- **Simplifications:** ignore the banned line; it's in the cube.

### Chromatic Star
- **Cost / type:** {1} — Artifact
- **Effect:** {1}, {T}, Sacrifice this artifact: Add one mana of any color. When this artifact is put into a graveyard from the battlefield, draw a card.
- **Gotchas:** Costs 1 to cast and 1 to activate, for one mana back plus a draw. It's a color filter that cantrips. The draw triggers no matter how it hits the graveyard. Mental Misstep target.
- **Simplifications:**

### Wizard's Rockets
- **Cost / type:** {1} — Artifact
- **Effect:** This artifact enters tapped. {X}, {T}, Sacrifice this artifact: Add X mana in any combination of colors. When this artifact is put into a graveyard from the battlefield, draw a card.
- **Gotchas:** It enters tapped, so it can't be activated the turn it's cast; it only works if it starts the puzzle on the battlefield. With X=0 it's a free draw; with X>0 it filters colors. The draw triggers however it reaches the graveyard.
- **Simplifications:**

### Cabal Ritual
- **Cost / type:** {1}{B} — Instant
- **Effect:** Add {B}{B}{B}. Threshold — Add {B}{B}{B}{B}{B} instead if there are seven or more cards in your graveyard.
- **Gotchas:** Net +1, or +3 with threshold, checked on resolution. Doomsday just emptied your graveyard down to itself, so reaching seven mid-pile is a counting puzzle. LED's discard, Frantic Search, surveil, and cracked artifacts all help.
- **Simplifications:**

### Mox Sapphire
- **Cost / type:** {0} — Artifact
- **Effect:** {T}: Add {U}.
- **Simplifications:**

### Mox Jet
- **Cost / type:** {0} — Artifact
- **Effect:** {T}: Add {B}.
- **Simplifications:**

### Sol Ring
- **Cost / type:** {1} — Artifact
- **Effect:** {T}: Add {C}{C}.
- **Gotchas:** Colorless only, so it can't pay {U}{U} for Oracle or {B}{B}{B} for Doomsday. Good for puzzles with enough mana but the wrong colors.
- **Simplifications:**

### Mishra's Bauble
- **Cost / type:** {0} — Artifact
- **Effect:** {T}, Sacrifice this artifact: Look at the top card of target player's library. Draw a card at the beginning of the next turn's upkeep.
- **Gotchas:** The draw never happens in a one-turn puzzle. Its uses are as a free spell for storm and as a free card into the graveyard for threshold.
- **Simplifications:** ignore the delayed draw and the look.

---

## Lands

### Basic lands (5)
| Card | Type | Mana | Fetched by |
|---|---|---|---|
| Plains | Basic Land — Plains | {W} | Flooded Strand |
| Island | Basic Land — Island | {U} | Polluted Delta, Flooded Strand |
| Swamp | Basic Land — Swamp | {B} | Polluted Delta |
| Mountain | Basic Land — Mountain | {R} | — |
| Forest | Basic Land — Forest | {G} | — |

- **Effect:** {T}: Add one mana of the listed color.
- **Gotchas:** The only lands Edge of Autumn can find. Under an opponent's Archon of Emeria they still enter untapped. Surgical Extraction can't target a basic. Island counts for Daze and Gush; Swamp for Snuff Out.
- **Simplifications:**

### Original dual lands (10)
| Card | Types | Mana | Fetched by |
|---|---|---|---|
| Underground Sea | Island Swamp | {U} or {B} | Polluted Delta, Flooded Strand |
| Tundra | Plains Island | {W} or {U} | Polluted Delta, Flooded Strand |
| Volcanic Island | Island Mountain | {U} or {R} | Polluted Delta, Flooded Strand |
| Tropical Island | Forest Island | {G} or {U} | Polluted Delta, Flooded Strand |
| Scrubland | Plains Swamp | {W} or {B} | Polluted Delta, Flooded Strand |
| Badlands | Swamp Mountain | {B} or {R} | Polluted Delta |
| Bayou | Swamp Forest | {B} or {G} | Polluted Delta |
| Plateau | Mountain Plains | {R} or {W} | Flooded Strand |
| Savannah | Forest Plains | {G} or {W} | Flooded Strand |
| Taiga | Mountain Forest | {R} or {G} | — |

- **Effect:** {T}: Add either listed color. (Oracle text is just the reminder of their basic land types.)
- **Gotchas:** They have basic land *types* but aren't basic, so Edge of Autumn can't find them and an opponent's Archon makes them enter tapped. Each counts as every type it lists: an Island for Daze and Gush, a Swamp for Snuff Out, and fetchable by any fetchland naming either type. The red duals are now the main way to cast Reckless Impulse, Pyroblast and Manamorphose without a Lotus or LED.
- **Simplifications:** the engine can derive mana and fetchability from the land's subtypes instead of hard-coding each card.

### Undercity Sewers
- **Cost / type:** Land — Island Swamp
- **Effect:** {T}: Add {U} or {B}. This land enters tapped. When this land enters, surveil 1.
- **Gotchas:** Playing it with your land drop can mill one card off the pile for free, but it gives no mana this turn. Fetching it out of the pile also triggers surveil.
- **Simplifications:**

### Meticulous Archive
- **Cost / type:** Land — Plains Island
- **Effect:** {T}: Add {W} or {U}. This land enters tapped. When this land enters, surveil 1.
- **Gotchas:** Same as Undercity Sewers.
- **Simplifications:**

### Polluted Delta
- **Cost / type:** Land
- **Effect:** {T}, Pay 1 life, Sacrifice this land: Search your library for an Island or Swamp card, put it onto the battlefield, then shuffle.
- **Gotchas:** It can fetch a land *out of the pile*. That shrinks the library and gives untapped mana (a basic or an original dual) or a surveil (Sewers, Archive). The shuffle afterward randomizes what's left. Costs 1 life. Fetches by land *type*, so any dual with Island or Swamp counts (see the dual-land table).
- **Simplifications:** see "Shuffles" in the engine-wide notes.

### Flooded Strand
- **Cost / type:** Land
- **Effect:** {T}, Pay 1 life, Sacrifice this land: Search your library for a Plains or Island card, put it onto the battlefield, then shuffle.
- **Gotchas:** Same as Polluted Delta, but for Plains or Island.
- **Simplifications:** see "Shuffles".

### Cavern of Souls
- **Cost / type:** Land
- **Effect:** As this land enters, choose a creature type. {T}: Add {C}. {T}: Add one mana of any color. Spend this mana only to cast a creature spell of the chosen type, and that spell can't be countered.
- **Gotchas:** Wizard covers both Oracle and Thundertrap Trainer. The colored mana can only pay for creature spells of the chosen type; otherwise it makes {C}. Spending even one Cavern mana on the spell makes it uncounterable.
- **Simplifications:** if it starts in play, the puzzle file sets its type.

---

## Opponent's board (static hate)

These start the puzzle on the opponent's battlefield. Static effects need no opponent decisions, so they're the simplest way to vary puzzles. Each one closes off some lines and leaves others open, which is the point.

### Narset, Parter of Veils
- **Cost / type:** {1}{U}{U} — Legendary Planeswalker — Narset (loyalty 5)
- **Effect:** Each opponent can't draw more than one card each turn. −2: Look at the top four cards of your library. You may reveal a noncreature, nonland card from among them and put it into your hand. Put the rest on the bottom of your library in a random order.
- **Gotchas:** Counts draws from earlier in the turn, so if you drew in your draw step (see "Turn history"), you can't draw at all. Extra draws are simply ignored; a "draw 3" draws 0 or 1. Scryfall ruling: Lab Man and Jace *can't* replace a draw Narset forbids, so drawing from an empty library doesn't win. Lines that still work: Oracle with enough devotion, emptying the library by surveil, mill (Jace +1's mill still happens), Reckless Impulse's exile, fetching, Edge of Autumn's search; and Doctor Doom, whose win check happens even if the draw is ignored. Ponder and Brainstorm still reorder.
- **Simplifications:** the −2 never matters (it's the opponent's).

### Orcish Bowmasters
- **Cost / type:** {1}{B} — Creature — Orc Archer. Flash.
- **Effect:** When this creature enters and whenever an opponent draws a card except the first one they draw in each of their draw steps, this creature deals 1 damage to any target. Then amass Orcs 1.
- **Gotchas:** The exception is only for the draw *step*, and the puzzle is in your main phase, so *every* card you draw pings. A draw replaced by Lab Man or Jace never happened, so it doesn't ping (and you've won anyway). Damage is dealt as each trigger resolves, so it adds to the life budget alongside Doomsday's halving, Probe, Wraith and fetches. You lose when the state-based check sees 0 life, so the timing of the last ping relative to the win matters.
- **Simplifications:** the target is an opponent choice and becomes a search branch: you, or one of your creatures (Lab Man is a 2/2, so two pings kill it). This needs creature toughness and damage in the state. Ignore the Orc Army and its token.

### Leyline of the Void
- **Cost / type:** {2}{B}{B} — Enchantment
- **Effect:** If this card is in your opening hand, you may begin the game with it on the battlefield. If a card would be put into an opponent's graveyard from anywhere, exile it instead.
- **Gotchas:** Your graveyard stays empty all turn, including Doomsday itself. Off: Unearth, Yawgmoth's Will, Flow State's two-card mode, Cabal Ritual's threshold, Tamiyo's −3. Also off: the draws from Chromatic Star and Wizard's Rockets, which trigger on going to the graveyard; they're exiled instead, so nothing triggers. Still works: surveil and mill still remove cards from the library (into exile), and LED's discard still empties your hand.
- **Simplifications:**

### Grafdigger's Cage
- **Cost / type:** {1} — Artifact
- **Effect:** Creature cards in graveyards and libraries can't enter the battlefield. Players can't cast spells from graveyards or libraries.
- **Gotchas:** Symmetric, but only hurts you here. Off: Unearth (the target can't enter), casting spells from the graveyard under Yawgmoth's Will. Scryfall ruling: you can still *play lands* from your graveyard under Will. Cards exiled by Reckless Impulse are cast from exile, so they still work.
- **Simplifications:**

### Thalia, Guardian of Thraben
- **Cost / type:** {1}{W} — Legendary Creature — Human Soldier
- **Effect:** First strike. Noncreature spells cost {1} more to cast.
- **Gotchas:** Applies to alternative costs too, so "free" spells cost {1}: Force of Will, Gush, Daze, Pact, life-paid Probe. Evoked Grief is a creature spell, so it stays free. Artifacts are noncreature spells, so LED, Lotus, Petal and Moxen cost {1}. Creature spells (Oracle, Lab Man, Riddler, Tamiyo) and all abilities (cycling, equip, LED's activation) are unaffected. Symmetric, so it also taxes the opponent's counterspells.
- **Simplifications:** ignore first strike.

### Archon of Emeria
- **Cost / type:** {2}{W} — Creature — Archon
- **Effect:** Flying. Each player can't cast more than one spell each turn. Nonbasic lands your opponents control enter tapped.
- **Gotchas:** Looks at the whole turn, so Doomsday was your one spell; you can't cast anything else. Wins have to come from abilities and lands: cycling (Street Wraith, Edge of Autumn, Miscalculation, Unearth), artifacts already in play (LED, Chromatic Star, Rockets, Spellbomb), planeswalkers already in play, or Doctor Doom already in play. Your nonbasic lands enter tapped, so a fetchland you play this turn can't be activated, and fetched duals give no mana. Basics are unaffected. It also limits the opponent to one spell on your turn.
- **Simplifications:** ignore flying.

---

## Protection and disruption

These can be in your hand, the opponent's hand, or both. The opponent casts them only in response (see Decisions). In your hand they matter as protection, as pitch fodder, or as dead cards; discard (Thoughtseize, Duress, Grief) can strip the opponent's interaction before you start the pile.

### Force of Will
- **Cost / type:** {3}{U}{U} — Instant. Alt: pay 1 life and exile a blue card from your hand.
- **Effect:** Counter target spell.
- **Gotchas:** The pitched card must be blue and comes from your hand, so it can eat a pile card you just drew. The alternative cost works on any turn.
- **Simplifications:**

### Force of Negation
- **Cost / type:** {1}{U}{U} — Instant. Alt: exile a blue card from your hand, only if it's not your turn.
- **Effect:** Counter target noncreature spell. If that spell is countered this way, exile it instead of putting it into its owner's graveyard.
- **Gotchas:** On *your* turn you must hardcast it for 1UU; the opponent can pitch it for free. It can't counter creatures such as Oracle, Trainer, Doom, or Grief.
- **Simplifications:**

### Mental Misstep
- **Cost / type:** {U/P} — Instant
- **Effect:** Counter target spell with mana value 1.
- **Gotchas:** Hits a large part of this pool: Ponder, Preordain, Consider, Brainstorm, Ancestral, Probe, Dark Ritual, Chromatic Star, Rockets, Spellbomb, and the 1-mana interaction. It doesn't hit 0-cost cards like LED, the Lotuses, or Gleemox.
- **Simplifications:**

### Counterspell
- **Cost / type:** {U}{U} — Instant
- **Effect:** Counter target spell.
- **Simplifications:**

### Miscalculation
- **Cost / type:** {1}{U} — Instant. Cycling {2}.
- **Effect:** Counter target spell unless its controller pays {2}.
- **Gotchas:** Cycling for {2} is a mana-costly draw on your side.
- **Simplifications:** the opponent never cycles it (that would be proactive). The {2} is paid from your floating mana, see Decisions.

### Daze
- **Cost / type:** {1}{U} — Instant. Alt: return an Island you control to its owner's hand.
- **Effect:** Counter target spell unless its controller pays {1}.
- **Gotchas:** The returned land must be an Island: Island, Underground Sea, Tundra, Volcanic Island, Tropical Island, Undercity Sewers, Meticulous Archive. A returned land can be replayed only if a land drop remains. Leaving {1} open beats it.
- **Simplifications:** the {1} is paid from your floating mana: tap the land in response, then pay when Daze resolves (see Decisions). The opponent may tap lands on the spot to pay for your Daze.

### Pact of Negation
- **Cost / type:** {0} — Instant
- **Effect:** Counter target spell. At the beginning of your next upkeep, pay {3}{U}{U}. If you don't, you lose the game.
- **Gotchas:** If you cast it on the turn you win, the upkeep cost never matters, so it's a free counter for you. It's blue (color indicator), so it can be pitched to Force of Will or Force of Negation.
- **Simplifications:** ignore the upkeep trigger.

### Flusterstorm
- **Cost / type:** {U} — Instant. Storm.
- **Effect:** Counter target instant or sorcery spell unless its controller pays {1}. Storm (copy it for each spell cast before it this turn).
- **Gotchas:** Needs the storm count. Each copy is a separate {1} tax, so it scales with how many spells you've cast. It can't hit creatures, artifacts, or abilities.
- **Simplifications:**

### Spell Pierce
- **Cost / type:** {U} — Instant
- **Effect:** Counter target noncreature spell unless its controller pays {2}.
- **Simplifications:** the {2} is paid from your floating mana, see Decisions.

### Pyroblast
- **Cost / type:** {R} — Instant
- **Effect:** Choose one — Counter target spell if it's blue. / Destroy target permanent if it's blue.
- **Gotchas:** Can destroy Oracle with its trigger on the stack, or Jace or Teferi. Can also counter your blue counterspell.
- **Simplifications:**

### Thoughtseize
- **Cost / type:** {B} — Sorcery
- **Effect:** Target player reveals their hand. You choose a nonland card from it. That player discards that card. You lose 2 life.
- **Gotchas:** Costs 2 life on top of the mana.
- **Simplifications:**

### Duress
- **Cost / type:** {B} — Sorcery
- **Effect:** Target opponent reveals their hand. You choose a noncreature, nonland card from it. That player discards that card.
- **Simplifications:**

### Grief
- **Cost / type:** {2}{B}{B} — Creature — Elemental Incarnation. Evoke—exile a black card from your hand.
- **Effect:** Menace. When this creature enters, target opponent reveals their hand. You choose a nonland card from it. That player discards that card.
- **Gotchas:** Evoke is free but pitches a black card. It's a creature spell, so Force of Negation and Spell Pierce can't counter it.
- **Simplifications:** ignore menace.

### Teferi, Time Raveler
- **Cost / type:** {1}{W}{U} — Legendary Planeswalker — Teferi (starting loyalty 4)
- **Effect:** Each opponent can cast spells only any time they could cast a sorcery. +1: Until your next turn, you may cast sorcery spells as though they had flash. −3: Return up to one target artifact, creature, or enchantment to its owner's hand. Draw a card.
- **Gotchas:** Once he resolves, the opponent can't cast anything on your turn. −3 also draws a card, so it can draw from the pile (with no target if needed). Adds 1 devotion. Loyalty abilities are sorcery speed, once per turn.
- **Simplifications:**

### Silence
- **Cost / type:** {W} — Instant
- **Effect:** Your opponents can't cast spells this turn.
- **Gotchas:** It can be countered in response, and it doesn't stop activated abilities.
- **Simplifications:**

### Veil of Summer
- **Cost / type:** {G} — Instant
- **Effect:** Draw a card if an opponent has cast a blue or black spell this turn. Spells you control can't be countered this turn. You and permanents you control gain hexproof from blue and from black until end of turn.
- **Gotchas:** It can be countered itself. After it resolves, blue counterspells can still be *cast* but don't counter your spells. Hexproof from blue/black doesn't stop Pyroblast or Swords.
- **Simplifications:**

### Defense Grid
- **Cost / type:** {2} — Artifact
- **Effect:** Each spell costs {3} more to cast except during its controller's turn.
- **Gotchas:** Symmetric no matter who controls it: on your turn, the opponent's spells cost 3 more, including alternative-cost Force of Will and Daze.
- **Simplifications:**

### Swords to Plowshares
- **Cost / type:** {W} — Instant
- **Effect:** Exile target creature. Its controller gains life equal to its power.
- **Gotchas:** Exiling Oracle with its trigger on the stack drops your devotion by 2.
- **Simplifications:**

### Snuff Out
- **Cost / type:** {3}{B} — Instant. Alt: pay 4 life, if you control a Swamp.
- **Effect:** Destroy target nonblack creature. It can't be regenerated.
- **Gotchas:** Can kill Oracle (blue) but not Doctor Doom or Grief (black).
- **Simplifications:**

### Stifle
- **Cost / type:** {U} — Instant
- **Effect:** Counter target activated or triggered ability. (Mana abilities can't be targeted.)
- **Gotchas:** The biggest threat to Oracle: it counters the enter trigger, and Cavern and Veil don't help because they protect spells, not abilities. Also hits cycling (the card is already discarded, no draw), fetchland activations (the fetch is already sacrificed), Doctor Doom's {T} ability, planeswalker loyalty abilities, Tamiyo's transform, Sewers/Archive surveil, and Chromatic Star/Rockets draw triggers. Can't touch LED, Lotus, Petal, Moxen or Sol Ring (mana abilities), or static replacements like Lab Man and Jace. Backup answer: a second win condition, or winning through a replacement effect.
- **Simplifications:**

### Consign to Memory
- **Cost / type:** {U} — Instant. Replicate {1}.
- **Effect:** Replicate {1} (copy it for each time you paid its replicate cost). Counter target triggered ability or colorless spell.
- **Gotchas:** Like Stifle for triggers (Oracle's trigger, cantrip triggers, surveil, Tamiyo), but it *can't* hit activated abilities, so cycling, fetching and Doom's tap are safe. In exchange it counters colorless *spells*: LED, Lotus, Petal, Moxen, Sol Ring, Bauble, Star, Rockets, Spellbomb, Gleemox, Greaves, Skateboard. Replicate lets it answer several things at once if the opponent has mana.
- **Simplifications:**

### Surgical Extraction
- **Cost / type:** {B/P} — Instant (pay {B} or 2 life)
- **Effect:** Choose target card in a graveyard other than a basic land card. Search its owner's graveyard, hand, and library for any number of cards with the same name as that card and exile them. Then that player shuffles.
- **Gotchas:** In response to Unearth, exiles Oracle or Lab Man from your graveyard. The bigger effect for piles: **you shuffle**, so any Surgical against you makes your library unordered, even if they target something irrelevant like Doomsday. It also reaches into your library and hand for same-named cards. Can't target basics.
- **Simplifications:**

### Hydroblast
- **Cost / type:** {U} — Instant
- **Effect:** Choose one — Counter target spell if it's red. / Destroy target permanent if it's red.
- **Gotchas:** Mirror of Pyroblast. Red spells in this pool: Pyroblast, Reckless Impulse, Manamorphose (red/green hybrid counts as red). Useful on your side to answer the opponent's Pyroblast on Oracle or Jace.
- **Simplifications:**

### Mindbreak Trap
- **Cost / type:** {2}{U}{U} — Instant — Trap
- **Effect:** If an opponent cast three or more spells this turn, you may pay {0} rather than pay this spell's mana cost. Exile any number of target spells.
- **Gotchas:** Doomsday counts, so after two more spells it's free for the opponent. Spells cast, not resolved, so countered spells count. It exiles rather than counters, so Cavern and Veil don't stop it. It can't touch abilities: once Oracle has *resolved*, its trigger is safe from Trap. It punishes long piles; the counter-strategy is winning in few spells or with abilities (cycling, Doom).
- **Simplifications:**

### Spell Snare
- **Cost / type:** {U} — Instant
- **Effect:** Counter target spell with mana value 2.
- **Gotchas:** Hits Thassa's Oracle, plus Night's Whisper, Ideas Unbound, Flow State, Cabal Ritual, Manamorphose, Reckless Impulse, Edge of Autumn (cast), Thundertrap Trainer, Lightning Greaves, Defense Grid, Daze and Miscalculation. Not Gush or Quantum Riddler (mana value 5 even when the alternative cost or warp is paid). Cavern on Oracle beats it.
- **Simplifications:**