# Fine Print — Software Requirements Specification (SRS)
## Deterministic TCG Rules Engine & State Machine (MTG Comprehensive Rules Parity)

* **Project Name:** **Fine Print**, the rules engine that reads what the card actually says.
* **Document ID:** `SRS-TCG-ENGINE-001`
* **Rules Baseline:** *Magic: The Gathering* Comprehensive Rules (CR), **effective November 6, 2026**. This baseline includes the *Foundations* combat changes (removal of damage assignment order), the *Edge of Eternities* Station/Spacecraft and Commander-eligibility rules, and the 2026 mechanics from *Lorwyn Eclipsed* (blight), *Avatar: The Last Airbender* (waterbend), *Secrets of Strixhaven* (preparation cards, increment, paradigm) and *Reality Fracture* (Empower Jace, Heartwood tokens).
* **Architecture Paradigm:** Standalone, embeddable, data-driven, event-sourced, deterministic state machine. Card behavior is declarative, and a separate offline toolchain compiles it from card data.

---

## 0. Document Conventions & Revision Summary

### 0.1 Normative Language
The key words **SHALL**, **SHALL NOT**, **SHOULD**, **SHOULD NOT** and **MAY** follow RFC 2119. Each requirement carries a stable ID (`REQ-<AREA>-<NNN>`). IDs are never reused: a removed requirement stays as a one-line entry marked **[WITHDRAWN]**. This document is not versioned; its change history is the repository's git history.

### 0.2 Rule Citations
* Every CR citation uses the numbering of the Rules Baseline above. The November 2026 CR inserted rule 713 (Meld Cards), which shifted the numbers of later sections in the 7xx range.
* **REQ-DOC-001 — Citation CI.** The repository SHALL contain a machine-readable index of every CR citation in this SRS and in source code comments. On each CR release, a CI job SHALL download the official CR text, check that every cited rule number still exists, and flag any citation whose rule text has changed since the last verified baseline. A changed rule SHALL block release until a maintainer re-verifies it.

---

## 1. Executive Summary & Foundational Principles

### 1.1 Purpose
This document specifies the functional, architectural and interface requirements for **Fine Print**, a clean-room, deterministic rules engine that executes the game mechanics, card layouts, continuous-effect interactions and format rules of *Magic: The Gathering*, as defined by the Rules Baseline.

The engine is a **standalone, UI-agnostic library and runtime** with a strictly typed public interface. Any consumer can use it to drive a complete match, inspect omniscient or player-redacted state, and use sandbox control. Consumers include a 3D desktop client, a web frontend, a tournament server, an MCTS agent and a headless test runner.

### 1.2 Scope of "Parity" (`REQ-ARCH-000`)
"100% parity" is defined at two separate levels, and the engine SHALL report each one separately:

1. **Rules parity.** The engine correctly implements every generic rule in the Rules Baseline: turn structure, priority, casting, combat, layers, replacement and prevention, triggers, SBAs, zones, layouts, designations and variants. This is a property of the engine binary.
2. **Card coverage.** Each individual card has a definition that correctly expresses its Oracle text in the §14 DSL. This is a property of the **card database**, which is a separate, versioned artifact (§16). Card coverage is never assumed. Every card definition carries a `supportStatus`, and the engine SHALL NOT silently run a card whose definition is missing or known to be incomplete (REQ-ING-008).

### 1.3 Legal & Architectural Decoupling (`REQ-ARCH-001`)
1. **Zero hardcoded IP.** The engine binary SHALL contain no card names, Oracle text, flavor text, mana symbol graphics, artwork or set icons. Generic rules vocabulary that the CR itself defines (keyword names, predefined token definitions from CR 111.10, counter kinds, designations) MAY be built into the engine. Card-specific content MAY NOT.
2. **Data-driven ingestion.** Card definitions, token definitions, format ban lists and FormatConfigs SHALL be supplied at runtime as versioned data artifacts (§14, §16).
3. **Licensing boundary (non-normative guidance; not legal advice).** Game mechanics are generally not protected by copyright, but card names, Oracle text, art and trademarks belong to Wizards of the Coast. Card data obtained from Scryfall is subject to Scryfall's API terms. These terms allow use for Magic software under the Wizards Fan Content Policy, require that access not be paywalled, and require that the software add value rather than simply republish the data. The Fan Content Policy requires free access and an "unofficial fan content" disclaimer, and it restricts commercial licensing. **Any commercial distribution of a product that bundles card data SHALL receive legal review and, where needed, a license from Wizards of the Coast before release.**

### 1.4 Naming Conventions (`REQ-ARCH-003`)
1. The product name is **Fine Print** and the machine identifier is `fineprint`. These SHALL be used consistently:

| Artifact | Name |
| :--- | :--- |
| Core library (Rust crate / .NET assembly / C header / WASM module) | `fineprint-core` / `FinePrint.Core` / `fineprint.h` / `fineprint.wasm` |
| Public API namespace (§13.2) | `FinePrint` (e.g. `FinePrint.IRulesEngine`) |
| Offline card toolchain (§16) | `fineprint-ingest` |
| Card database artifact (REQ-ING-014) | `*.fpdb` (`CardDatabaseArtifact`) |
| Puzzle/test scenario files (REQ-TEST-001) | `*.pzl` (unchanged) |
| Replay logs (REQ-API-005) | `*.fpreplay` |

2. The product name, identifiers, logos and UI branding SHALL NOT include third-party game names, trademarks or logos (consistent with REQ-ARCH-001). Descriptive references to the game that the engine supports stay confined to documentation and compatibility notes. Any such reference MUST carry the disclaimer required by §1.3.

### 1.5 Foundational Meta-Rules (`REQ-ARCH-002`)
The engine SHALL enforce the CR 101 "golden rules" as cross-cutting constraints:
* **CR 101.1 — Card precedence.** When a card's text directly contradicts a rule, the card takes precedence, but only over the specific rule it contradicts. Architecturally, every generic procedure exposes **query points** where card-contributed effects are consulted (§14.6), so no rule is hardcoded in a way a card cannot override.
* **CR 101.2 — "Can't" beats "can".** A prohibition always beats a permission or requirement. Every query point that merges permissions with prohibitions SHALL evaluate prohibitions last and let them win.
* **CR 101.3 — Impossible instructions.** Any part of an instruction that is impossible to perform is ignored. The rest of the instruction is still carried out.
* **CR 101.4 — APNAP.** When multiple players make choices or take actions at the same time, the active player goes first, then the others in turn order. This also governs simultaneous timestamps (CR 613.7m) and the ordering of simultaneous triggers (CR 603.3b).

---

## 2. Core Data Model & State Management

### 2.1 Deterministic State Tree (`REQ-CORE-001`)
The entire match state at tick $t$ is an immutable (or snapshot-serializable) value tree $S_t$. Every transition satisfies
$$S_{t+1} = f(S_t, \text{Action}_t)$$
where the PRNG state is part of $S_t$.
* **No hidden side effects.** Rules code, card ASTs and native effects (REQ-DSL-015) SHALL NOT read or write anything outside $S_t$, including wall-clock time, global mutable state and I/O.
* **PRNG.** All randomness SHALL come from a single, specified PRNG algorithm stored in $S_t$. This covers shuffles, coin flips, die rolls, random discards, random selection, `Seek`, and hidden-information resampling during forks. The algorithm is fixed per engine major version (default: `PCG64-DXSM`), and its seed derivation is specified in REQ-API-004.
* **REQ-CORE-006 — Cross-platform determinism.** Rules paths SHALL NOT use floating-point arithmetic. Numbers SHALL be signed 64-bit integers. Half-values (e.g. half-mana symbols in Un-sets) SHALL use fixed-point with scale 2. Iteration over any collection whose order can affect the outcome SHALL use an ordered container or an explicit sort on a stable key (ObjectId, then timestamp). Language-default hash-map iteration order is forbidden. Every language binding (the Rust API, the C ABI, .NET and WASM; REQ-NFR-002) SHALL replay the shared golden replay corpus to identical state hashes in CI on every platform in REQ-NFR-003.

### 2.2 Entity Identity Hierarchy (`REQ-CORE-002`)
| Identifier | Lifecycle Scope | Purpose & CR Mapping |
| :--- | :--- | :--- |
| **`CardId`** | Entire match | The physical card, or a conjured card (§11.5). Persists across zone changes. Holds `OwnerId`, `CardDefinitionRef` (definition ID and definition hash), Commander designation (CR 903.3, an attribute of the card, not a characteristic), stickers (CR 123), and extension data such as perpetual modifications (§11.5). |
| **`ObjectId`** | One zone residency | Assigned whenever an object enters a zone. Per **CR 400.7**, a zone change creates a new object with no memory of the old one, apart from the explicit exceptions in **CR 400.7a–m**. Each exception SHALL be implemented as a named, tested rule (e.g. 400.7d, which lets a permanent see the costs paid for the spell it came from, and 400.7j, which lets an effect find an object it moved to a public zone). A merged permanent (CR 731) is one ObjectId represented by several CardIds. |
| **`AbilityInstanceId`** | Stack residency, or pending trigger | Identifies a triggered or activated ability on the stack or waiting to go on the stack. Records its source ObjectId (or a "sourceless" marker for inherent abilities such as Monarch, Initiative, rad counters and speed), its controller, and its bound variables. |
| **`EffectId`** | Lifetime of a continuous effect, replacement effect or delayed trigger | Stable handle for dependency analysis, `AppliedEffects` tracking (CR 614.5) and UI breakdowns. |
| **`Timestamp`** | Per object, effect, counter or attachment | A `uint64` from a single monotonic counter, assigned per **CR 613.7a–n**. Includes 613.7a (static abilities take the later of the object's and the granting effect's timestamp), 613.7c (counters), 613.7e (attachment), 613.7f (face up/down), 613.7g (transform/convert) and 613.7m (simultaneous timestamps assigned in APNAP order, with each player choosing the order among their own). |
| **`LinkId`** | Lifetime of a linked-ability pair | Key into the per-object Linked Store (§2.6, CR 607). |

### 2.3 Last Known Information (`REQ-CORE-003`)
When an object leaves a zone, or is moved from a public zone to a hidden zone, the engine SHALL capture an immutable `LKIRecord` of its fully evaluated characteristics (layers 1–7, after the 613.10/613.11 passes where relevant). The record includes controller, counters, attachments, status, abilities and per-object memory, all as of immediately before the event.
* Per **CR 608.2h**, an effect that needs information about a specific object uses its current information while the object is still in the public zone the effect expects, and otherwise uses the LKI.
* Per **CR 608.2b**, target legality checks for an ability whose source has left its zone use the source's LKI.
* Look-back triggers (§8.2) evaluate against LKI captured in the pre-event state.

### 2.4 Information Redaction & Knowledge Model (`REQ-CORE-004`)
The engine SHALL provide the projection
$$V(S_t, P_i) \rightarrow S_{t, P_i}^{\text{redacted}}$$
which contains exactly the information player $P_i$ is entitled to.
* **Identity knowledge.** Each object in a hidden zone, and each face-down object, carries a `KnownTo: PlayerSet` recording who knows its identity (because it was revealed, looked at, or seen in a public zone and tracked since).
* **Position knowledge.** Libraries carry `KnownPositions: Map<PlayerId, Map<index, ObjectId>>`, so knowledge such as "Player 1 knows the top two cards after Brainstorm" survives until the order changes.
* **Shuffle.** Shuffling a library clears all position knowledge for that library. It does not clear identity knowledge of the library's contents as a set; for example, a player who revealed a card and then shuffled it in still knows that card is in the library. A redacted library view SHALL expose that set knowledge, never position.
* **Face-down rules.** A controller may look at face-down permanents and spells they control (CR 708.5). Face-down cards that leave the battlefield or stack are revealed per CR 708.9. Face-down exiled cards stay hidden, along with whatever permissions let specific players look at them.

### 2.5 Transactional Rollback (`REQ-CORE-005`) (CR 734)
Before a player begins to cast a spell, activate an ability or take a special action, the engine SHALL take a checkpoint $S_{\text{checkpoint}}$. If the action turns out to be illegal, or can't be completed (for example, because the cost can't be paid in 601.2h), the engine SHALL restore the checkpoint (CR 734.1). The player who had priority keeps it (CR 734.2).
* **Mana-ability reversal limits (CR 734.1).** Mana abilities activated during the action are reversed with it, except in two cases. (a) Mana from them, or from triggered mana abilities they caused, was spent on another mana ability that isn't being reversed. (b) They moved cards to a library, moved cards from a library to any zone other than the stack, shuffled a library, or revealed cards from a library. Mana abilities in category (b) SHALL be committed as their own sub-transactions, so that rolling back the outer action leaves them in place.
* **Engine-level undo** (sandbox `RewindToSnapshot`, REQ-API-003) is a separate feature. It is not governed by CR 734 and is never exposed in rules-enforced matches unless FormatConfig allows it.

### 2.6 Per-Object Memory & Linked Store (`REQ-CORE-007`)
Each ObjectId SHALL carry a `Memory` record that is serialized with the state. It holds:
* **Choices made as the object entered or resolved:** chosen color, creature type, card name, player, number, mode, or an anchor word (CR 614.12c).
* **Spell history visible to the permanent (CR 400.7d):** costs paid, mana spent (by color and source type), whether it was kicked, gifted or warped, the value of X, and the face or characteristics it was cast with (prototype, Adventure, Omen and so on).
* **Linked Store (CR 607):** `Map<LinkId, List<ObjectRef | PlayerRef | Value>>`. This records which objects or actions one linked ability produced, for its partner ability to refer back to ("the exiled card", "cards exiled with this", "the chosen creature type"). Each entry stores a durable reference (`CardId` plus the ObjectId generation at the moment of linking) so it can be resolved after zone changes where CR 400.7 allows. Linking across objects (CR 607.1d) is supported by storing the link on the source object and copying the `LinkId` to the created object.

There is no separate exile-link registry with hardcoded tags (`Imprint`, `Foretell`, `Plot`…): this generic mechanism covers them, and keyword macros (§14.7) allocate their own LinkIds.

---

## 3. Zone Topology (CR 400–408)

### 3.1 Standard & Variant Zones (`REQ-ZONE-001`)
1. **Library (CR 401, per player).** Ordered sequence (index 0 is the top). Hidden. Supports "play with the top card revealed" and "may look at the top card any time" static abilities. Per **CR 401.5**, these are re-evaluated each time the top card changes, including partway through casting or resolution.
2. **Hand (CR 402, per player).** Unordered. Visible to its owner. Maximum hand size (default 7) is a player property computed in the **613.11 rule-modification pass** (§6.4). There is no "Layer 8".
3. **Battlefield (CR 403, shared).** Each permanent tracks:
   * `ControllerId`, the result of layer 2.
   * **Status (CR 110.5):** tapped/untapped, flipped/unflipped, face up/face down, phased in/phased out.
   * **Designations:** e.g. `LeftHalfUnlocked`/`RightHalfUnlocked` (CR 709.5c), `Prepared` (CR 723.3a), `Goaded{byPlayers}` (CR 701.15), `RingBearer`, `Monstrous`, `Renowned`, `Suspected`, `Saddled`, and sector designations (CR 702.158). Designations are an open, data-defined enum, so new sets can add them without engine changes.
   * **Continuous-control tracking.** The turn number since which the permanent has been under its controller's continuous control. This supports the CR 302.6 summoning-sickness check, which is "has haste OR has been under continuous control since the controller's most recent turn began". It is not a stored boolean.
   * **Attachments.** Each permanent records what it is attached to (a permanent or a player). The engine SHALL enforce that attachment relationships never form a cycle.
   * **Combat state:** attacking (and what it is attacking), blocking (and what it blocks), blocked or unblocked, and removed from combat.
4. **Graveyard (CR 404, per player).** Ordered (index 0 is the top). Public. Order SHALL be preserved and chosen per the CR (e.g. CR 731.3a lets the owner choose the order of merged components). An optional FormatConfig flag MAY allow free reordering for formats that ignore graveyard order.
5. **Stack (CR 405, shared).** Ordered. Public, except for face-down spells. Holds spells, copies of spells, and ability instances.
6. **Exile (CR 406, shared).** Unordered. Public, except for face-down cards, whose visibility follows each card's `KnownTo`. Cards linked to an ability are tracked through the Linked Store (§2.6), not through exile-specific tags.
7. **Command zone (CR 408, shared).** Holds commanders, emblems, dungeons, planes and phenomena, schemes, vanguards and conspiracies. It holds only objects the CR places there. For example, the Empower Jace planeswalker token (CR 701.71a) is created on the battlefield, not here.
8. **Ante (CR 407).** Supported for completeness. Disabled by default in FormatConfig.
9. **Outside the game.** Sideboards, companions, wish targets, Lesson pools (Learn, CR 701.48), Attraction decks (CR 718), Contraption decks, spellbooks (digital extension) and the subgame outside-the-subgame set (CR 730.4).

### 3.2 Subgames (`REQ-ZONE-002`) (CR 730)
* Starting a subgame SHALL push the parent `GameState` onto a call stack and create a child state with an entirely new set of zones. Each player's main-game library becomes their subgame library and is shuffled (CR 730.2). Supplementary decks, vanguards and commanders move into the subgame as described in CR 730.2a–c. The starting player is chosen at random, and normal game start applies (CR 103), including the seven-card draw (CR 730.3).
* Objects and cards outside the subgame are "outside the game" from the subgame's point of view (CR 730.4).
* **When a subgame ends (CR 730.5):** each player puts every traditional card they own that is in the subgame into their **main-game library**, except cards in the subgame command zone. This includes cards in subgame exile and phased-out cards. Each player then shuffles. Supplementary decks, vanguards and commanders return as described in CR 730.5a–c. Every other subgame object stops existing. The parent game then finishes resolving the spell or ability that created the subgame.
* Subgames MAY nest (CR 730.6). The engine SHALL support a stack of parent states of any depth.

---

## 4. Object Anatomy & Card Layouts (CR 200–315, 708–723, 731)

### 4.1 Characteristics Vector (`REQ-OBJ-001`)
Each object, or face of a card, exposes the CR 109.3 characteristics:
* `Name`: a set of strings. A split card off the stack has two names. Some effects give objects no name.
* `ManaCost`: a structured multiset of symbols (generic, colored, colorless `{C}`, hybrid, monocolored hybrid `{2/W}`, Phyrexian, hybrid Phyrexian, snow, `{X}`/`{Y}`/`{Z}`), or **no mana cost**. "No mana cost" is different from `{0}`.
* `ManaValue`: derived per **CR 202.3**:
  * 202.3a: an object with no mana cost has MV 0, except back faces and melded permanents.
  * 202.3b: the back face of a nonmodal DFC uses the front face's mana cost. A *copy* of a back face has MV 0.
  * 202.3c: a melded permanent's MV is the combined mana cost of both front faces. A copy of a melded permanent has MV 0.
  * 202.3d: a split card's MV is the sum of both halves off the stack, or the cast half on the stack (both halves if fused).
  * 202.3e: X counts as 0 except on the stack.
  * 202.3f: hybrid symbols count their largest component.
  * 202.3g: each Phyrexian symbol counts as 1.
* `Colors`: a bitmask derived from the mana cost, color indicator, CDAs and effects (CR 105, 202.2).
* `ColorIdentity`: computed per **CR 903.4** for Commander validation. It is a property of the *card*, not of the in-game object.
* `Supertypes`: open set (Basic, Legendary, Snow, World, Ongoing, …).
* `CardTypes`: open set (Artifact, Battle, Conspiracy, Creature, Dungeon, Enchantment, Instant, Kindred, Land, Phenomenon, Plane, Planeswalker, Scheme, Sorcery, Vanguard). Must stay extensible, because new card types are rare but do happen.
* `Subtypes`: categorized by card type (CR 205.3). The full lists come from the card database's `RulesVocabulary` artifact, which the toolchain extracts from the CR. They are not hardcoded.
* `RulesText`: a sequence of `TextToken`s (§14.5) that Layer 3 can rewrite.
* `Abilities`: a list of ability ASTs (§14).
* `Power`/`Toughness`: an integer, a CDA expression, or absent. `Loyalty` (starting loyalty) and `Defense` (starting defense).
* `HandModifier`/`LifeModifier` (Vanguard).

### 4.2 Layout Behaviors (`REQ-OBJ-002`)
`CardDefinition.layout` SHALL be one of the following behaviors. Each one names the rules that decide which characteristics apply in which zone. All "alternative characteristics" below are **part of the object's copiable values**. The layout rules select them as the object's base characteristics before layer 1 effects are applied. None of them is a layer-1b effect.

| Layout | CR | Outside stack and battlefield | On the stack | On the battlefield | Key engine rules |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **NORMAL** | — | Printed | Printed | Printed | — |
| **SPLIT** | 709 | Both halves combined (two names, MV = sum) | Only the half chosen to cast; both if **fused** (702.102) | N/A (instant or sorcery) | 202.3d MV. Name choices: either half. |
| **ROOM** (split permanent with a shared type line) | 709.5 | Both halves | Only the cast half (the existence of both halves is copiable, 709.5b) | Only *unlocked* halves have their name, mana cost and text (709.5) | Enters with the cast half unlocked (709.5d). **Unlock is a special action** (116.2m, 709.5e): pay the locked half's mana cost, with the stack empty, in your main phase. Lock and unlock effects (709.5f–g). "Unlock" and "fully unlock" triggers (709.5h–i). |
| **FLIP** | 710 | Unflipped half | Unflipped half | Flipped half once `Flipped` status is set | Flipping is permanent until the object changes zones. Merged flip components follow 731.2h. |
| **LEVELER** | 711 | Printed | Printed | Level-band abilities, gated on level counters | Level up (702.87) is a sorcery-speed activated ability. |
| **DFC_NONMODAL** (transform/convert) | 712, 701.27, 701.28 | Front face | Front face, unless cast "transformed/converted" (712.8a, e.g. disturb, more than meets the eye) | The face that is up | 202.3b MV. A new timestamp on transform/convert (613.7g). Transforming does not create a new object (712.15, 701.27c). An ability can't transform its own permanent again if it has already transformed since the ability went on the stack (701.27g). Transforming into an instant or sorcery face does nothing (701.27e). Can't be turned face down (712.13). |
| **DFC_MODAL** | 712.3, 712.6e, 712.8b, 712.9 | Front face (712.6a) | The face chosen | The face chosen (can be played as a land if that face is a land) | MV from the face that's up, using its own mana cost. **Under the current CR, an MDFC *can* transform or convert if an ability tells it to (712.3, 701.27a)**, except into an instant or sorcery face (701.27e). |
| **MELD** | 713, 701.42 | Front face (meld cards are **not** DFCs, 713.1) | Front face | A melded pair is one object with the combined back face (713.1a) | 202.3c MV. Only the designated meld pair can meld (701.42b). When it leaves the battlefield, the two cards move separately. Meld cards can't transform (713.1c). |
| **SAGA** | 715 | Printed | Printed | Chapter abilities triggered by lore counters | A lore counter is added as a precombat-main turn-based action (505.4). SBA 704.5s. |
| **ADVENTURER** | 716 | Normal characteristics | Normal, or Adventure alternative characteristics (716.3) | Normal | A resolving Adventure is exiled with permission to cast it later, but not as an Adventure (716.3d). |
| **CLASS** | 717 | Printed | Printed | Level-gated abilities | Class levels are tracked as a designation value. |
| **ATTRACTION** | 718 | Attraction deck | — | Lit-number abilities | Visit/roll procedure. |
| **PROTOTYPE** | 719, 702.160 | Normal | Normal, or prototype mana cost/P/T (719.3) | Same as on the stack | The prototype values are alternative characteristics that are part of copiable values (719.2a). Colors come from the prototype mana cost (719.3b). Copies of prototyped spells and permanents are also prototyped (719.3c–d). |
| **CASE** | 720 | Printed | Printed | "Solved" designation gates abilities | — |
| **OMEN** | 721 | Normal | Normal, or Omen alternative characteristics (721.3) | Normal | **A resolving Omen spell is shuffled into its owner's library** (721.3d), not exiled. An Omen that is countered or otherwise leaves the stack follows normal rules. |
| **STATION** | 722, 702.184 | No P/T outside the battlefield (722.2c) | Printed | Each **station symbol `{N+}`** is a static ability: "As long as this has N or more charge counters, it has [abilities] (and is a creature with base P/T [P/T] in addition to its other types)" (722.2a–b) | The station keyword is an activated ability: tap another untapped creature you control, add charge counters equal to its power, sorcery speed (702.184a). Static effects may substitute another characteristic for power (702.184c). |
| **PREPARATION** | 723 | Normal only (723.4) | Normal only. The prepare spell **can't** be cast from the card itself (723.3) | Normal, plus the `Prepared` designation | Becoming prepared (or phasing in prepared) creates a copy in exile with only the prepare spell's characteristics. Only the **prepared permanent's current controller** may cast it. The copy is an exception to 704.5e and lasts while the permanent stays prepared on the battlefield. The permanent loses `Prepared` when the copy becomes cast (723.3c). A permanent that is already prepared can't become prepared again (723.3a). |
| **MUTATE** (merged permanents) | 702.140, 731 | Individual cards | A mutating creature spell targets a non-Human creature with the same owner | One object; **the top component supplies the characteristics** (731.2a, a layer-1a copiable effect timestamped at the merge), and the object has **all abilities of all components** (702.140e) | The merged permanent is not a new object and has not "entered" (731.2b–c). Rules for components that are tokens, face down or DFCs: 731.2d–j. Leaving the battlefield: 731.3a–e. |
| **TOKEN / DFC_TOKEN** | 111 | — | — | Per its definition | Predefined tokens: §10.1. |
| **PLANAR / SCHEME / VANGUARD / CONSPIRACY / DUNGEON** | 901–905, 309 | Command zone | — | — | Variant-gated. |

### 4.3 Face-Down Spells & Permanents (`REQ-OBJ-003`) (CR 708)
* A face-down object has **only** the characteristics listed by the rule or effect that turned it face down. Those characteristics are its copiable values (708.2), so face-down status is applied in **layer 1b** (613.2b).
* If nothing is listed, the object is a **2/2 creature with no text, no name, no subtypes and no mana cost** (708.2a). This is the default for morph (702.37), megamorph, manifest (701.40) and manifest dread (701.62).
* **Disguise (702.168) and cloak (701.58)** add **ward {2}** to that default.
* A card effect may specify other characteristics, for example an effect that turns a permanent face down as a 2/2 Cyberman artifact creature. The DSL represents this as a `FaceDownProfile` attached to the effect, not as an engine special case.
* Objects put onto the battlefield face down are turned face down first, so their ETB abilities don't trigger or apply (708.3). Spells cast face down are checked for prohibitions using their face-down characteristics (708.4).
* **Turning face up** is a special action (116.2b) for morph, megamorph and disguise (pay the cost) and for manifested or cloaked creature cards (pay the mana cost). When it turns face up, the object's copiable values revert (708.8) and "as … is turned face up" abilities apply during the turn (708.11). A face-down instant or sorcery in a merged permanent can't be turned face up (731.2g).
* DFCs can't be turned face down (712.13). A face-down DFC turns face up front face up (712.12a).

---

## 5. Turn Structure, Priority Loop & Combat (CR 117, 500–514)

### 5.1 The Priority & SBA Heartbeat (`REQ-TURN-001`)
The engine SHALL run the following loop whenever a player would receive priority (CR 117.3, 704.3):

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    PRIORITY HEARTBEAT STATE MACHINE                     │
└─────────────────────────────────────────────────────────────────────────┘
       ┌───► [1. Check SBAs (CR 704.3); perform ALL applicable ones
       │        simultaneously as one event; apply 704.7 replacement merge]
       │                     │ any performed? ── YES ──► repeat step 1
       │                     ▼ NO
       │     [2. Put waiting triggers on the stack (CR 603.3b):
       │        APNAP; each player orders their own; targets and modes
       │        are chosen as each one is put on the stack (603.3d)]
       │                     │ any put on stack? ── YES ──► go to step 1
       │                     ▼ NO
       └──── [3. Grant priority to PriorityPlayer]
                             │
              ┌──────────────┴───────────────────────────┐
         Takes action                                   Passes
     (cast / activate / special action)        (CR 117.3d: announce floating mana)
              │                                          │
   Execute through the transaction              PassCount += 1
   pipeline (§7). Acting player                  PassCount < PlayersInGame?
   RETAINS priority (117.3c).                     ├─ YES → next player in turn order
   PassCount = 0 → go to step 1                   │        → go to step 3
                                                  └─ NO (all passed in succession, 117.4)
                                                      ├─ Stack non-empty → resolve top
                                                      │   object; ActivePlayer gets
                                                      │   priority (117.3b); PassCount = 0;
                                                      │   go to step 1
                                                      └─ Stack empty → current step/phase
                                                          ends (500.5: "until end of step"
                                                          effects expire, then mana empties)
```
* **Mana abilities** (CR 605) do not use the stack and do not reset `PassCount` beyond the action they are part of. A player may activate them whenever they have priority, or whenever a payment is requested (605.3a).
* **Special actions** (CR 116.2a–m, twelve kinds) do not use the stack. The acting player retains priority (117.3c).
* **Players who have left the game** (CR 800.4j) are skipped. If the active player has left, priority goes to the next player in turn order.
* **Yields and auto-pass** (REQ-API-002 `setPlayerYields`) are a client convenience. The engine SHALL still record every pass explicitly in the replay log, so that a replay never depends on client yield settings.

### 5.2 Phase & Step Sequence (`REQ-TURN-002`)
At the end of every step and phase, effects that last until then expire, and then all unspent mana empties (**CR 500.5**). Effects that last "until [a step/phase]" expire as that step or phase begins (500.4). Effects that change how mana empties (e.g. mana that doesn't empty) are 613.11 rule modifications.

1. **Beginning phase (CR 501)**
   * **Untap step (CR 502).** No player receives priority (502.4). The turn-based actions happen in this order:
     1. **Phasing (502.1):** the active player's phased-in permanents with phasing phase out, and permanents that phased out under their control phase in, all simultaneously.
     2. **Day/night check (502.2, 732):** if it is day and the previous turn's active player cast no spells, it becomes night. If it is night and they cast two or more, it becomes day. Variant 502.2a applies to shared team turns.
     3. **Untap (502.3):** the active player decides which permanents to untap (subject to "doesn't untap" restrictions and stun-counter replacement, 122.1d), then untaps them simultaneously.
     4. Triggers during this step wait until the next time a player would receive priority (502.4).
   * **Upkeep step (CR 503):** triggers, then the heartbeat.
   * **Draw step (CR 504):** the active player draws a card as a turn-based action. The first-turn skip is governed by `FormatConfig.firstTurnDrawRule`, which defaults to the CR: skip in a two-player game (103.8a), the starting team skips in Two-Headed Giant (103.8b), and nobody skips in other multiplayer games (103.8c). Then triggers, then the heartbeat.
2. **Precombat main phase (CR 505).** Turn-based actions in CR order. These include, where applicable, setting an Archenemy scheme in motion, visiting Attractions, and putting a lore counter on each Saga with chapter abilities (505.4). Then triggers (including the rad-counter inherent trigger, CR 729.1), then the heartbeat. Only the first main phase of a turn is "precombat" (505.1a).
3. **Combat phase (CR 506)**
   * **Beginning of combat step (CR 507):** triggers, then the heartbeat.
   * **Declare attackers step (CR 508):**
     1. The active player submits a proposed `AttackDeclaration: Map<AttackerId, AttackTarget>`. An `AttackTarget` is a player, a planeswalker, or a battle the attacker may legally attack (a battle protected by an opponent, or as the battle's rules allow). In multiplayer games each attacker picks its own target, and range-of-influence limits apply.
     2. **Constraint solver (508.1c–d):** gather every `ATTACK_RESTRICTION`, `ATTACK_REQUIREMENT` and `ATTACK_COST` rule modification in force (§14.4.3) plus the evasion and keyword rules. A declaration is legal only if it violates no restriction **and** obeys the maximum possible number of requirements without breaking a restriction. Requirements that can only be met by paying a cost do not count as obligations (508.1d). Goad (701.15) supplies requirements of the form "attacks each combat if able, and attacks a player other than the goading player if able".
     3. **Costs (508.1h):** total all attack costs (e.g. Propaganda-style taxes, exert, enlist, saddle restrictions). Mana abilities may be activated, then the costs are paid. If they can't be paid, the declaration is rolled back per CR 734.
     4. Tap the attackers that don't have vigilance. Mark them as attacking. Triggers, then the heartbeat.
   * **Declare blockers step (CR 509):**
     1. Each defending player submits `BlockDeclaration: Map<BlockerId, List<AttackerId>>`.
     2. **Constraint solver (509.1b–c):** combine evasion rules (flying/reach 702.9/702.17, menace 702.111, shadow 702.28, horsemanship 702.31, fear 702.36, intimidate 702.13, skulk 702.118, landwalk 702.14, protection 702.16, "can't be blocked" statics), `BLOCK_RESTRICTION`, `BLOCK_REQUIREMENT` (e.g. "must be blocked if able") and `BLOCK_CAPACITY` (e.g. "can block an additional creature" or "any number"). Apply the same maximize-requirements rule as for attackers.
     3. **Costs (509.1d):** block costs work like attack costs.
     4. **No damage-assignment-order prompt.** The Nov. 2024 *Foundations* update removed damage assignment order.
     5. Each attacker with one or more blockers becomes blocked. Attackers with none become unblocked. "Becomes blocked" stays true even if all its blockers are later removed. Triggers, then the heartbeat.
   * **Combat damage step (CR 510):**
     * **First strike (510.4):** if any attacking or blocking creature has first strike or double strike as the step begins, there are two combat damage steps. The first involves only creatures with first strike or double strike. The second involves creatures that dealt no damage in the first step (unless they gained double strike since) plus creatures with double strike.
     1. **Assignment (510.1a–e):** the active player assigns damage for each attacker, then each defending player assigns damage for each blocker.
        * A creature assigns damage equal to its power. If that is 0 or less, it assigns none (510.1a).
        * An unblocked attacker damages the player, planeswalker or battle it is attacking, or nothing if that has left combat (510.1b).
        * A blocked attacker divides its damage **freely** among its blockers (510.1c). A blocker divides its damage freely among the attackers it blocks (510.1d).
        * **Trample (702.19b):** damage can go to the attacked player, planeswalker or battle only once every blocker has been assigned lethal damage. Lethal damage accounts for damage already marked and damage being assigned by other creatures in the same step, but not prevention or other effects. A source with deathtouch needs to assign only 1 for it to be lethal (702.2). **Trample over planeswalkers** (702.19c) lets excess go on to the planeswalker's controller.
        * **Banding (702.22):** a creature with banding lets its controller assign the damage instead of the normal assigning player, as specified.
        * After assignment, the whole assignment is validated (510.1e). If it is illegal, assignment is rolled back and redone.
        * The `DIVIDE_VALUE` choice (§15.3) SHALL carry the lethal-damage floor for each blocker whenever trample applies.
     2. **Dealing (510.2):** all combat damage is dealt simultaneously as one event. No player receives priority between assignment and dealing.
     3. Triggers (including look-back), then the heartbeat.
   * **End of combat step (CR 511):** triggers, then the heartbeat. As the step ends, all creatures, battles and planeswalkers are removed from combat (511.3). "Until end of combat" effects expire at the end of the combat *phase* (500.5a).
4. **Postcombat main phase (CR 505):** triggers, then the heartbeat. Additional combat phases and main phases created by effects follow 505.1a.
5. **Ending phase (CR 512)**
   * **End step (CR 513):** "at the beginning of the end step" triggers, then the heartbeat.
   * **Cleanup step (CR 514):**
     1. The active player discards down to their maximum hand size (514.1).
     2. Simultaneously, all marked damage is removed and "until end of turn" / "this turn" effects end (514.2).
     3. **No priority unless needed (514.3a):** check SBAs and waiting triggers. If there are none, the step ends without anyone receiving priority. Otherwise run the heartbeat, and once the stack is empty and all players pass, start **another cleanup step**.

### 5.3 Turn Control & Extra/Skipped Turns (`REQ-TURN-003`)
The engine SHALL support:
* extra turns, phases and steps, inserted LIFO per the CR;
* skipped steps and phases as 613.11 rule modifications;
* "end the turn" effects (CR 725), which exile the stack and so on, then run cleanup;
* controlling another player's turn or decisions (CR 724), where the controlling player makes every choice of the controlled player through the choice protocol and the redaction view is extended accordingly;
* restarting the game (CR 728).

---

## 6. Continuous Effects & the Layer System (CR 611–613)

### 6.1 Layer Evaluation Pipeline (`REQ-LAYER-001`)
Continuous effects SHALL never mutate an object's base definition. The engine computes an object's characteristics by starting from its base values and applying effects in this order (CR 613.1–613.4):

```
┌──────────────────────────────────────────────────────────────────────────┐
│               CR 613 CONTINUOUS EFFECTS PIPELINE (per object)            │
├────────────┬─────────────────────────────────────────────────────────────┤
│ Base       │ Printed characteristics of the face/half/alternative set in │
│            │ use (layout rules, §4.2), OR token/copy definition (613.1)  │
├────────────┼─────────────────────────────────────────────────────────────┤
│ Layer 1a   │ Copiable effects: copy effects (707), merging (731.2a),     │
│            │ "as enters / as turned face up" P/T-setting (613.2a)        │
│ Layer 1b   │ Face-down characteristics (708.2) — face-down ONLY          │
│ (613.2c)   │ → result = COPIABLE VALUES                                  │
├────────────┼─────────────────────────────────────────────────────────────┤
│ Layer 2    │ Control-changing effects                                    │
│ Layer 3    │ Text-changing effects (612) — rewrite TextTokens (§14.5)    │
│ Layer 4    │ Type/subtype/supertype-changing (incl. 305.7 basic types)   │
│ Layer 5    │ Color-changing effects                                      │
│ Layer 6    │ Ability adding/removing; keyword counters (122.1b);         │
│            │ "can't have [ability]"                                      │
│ Layer 7a   │ P/T characteristic-defining abilities                       │
│ Layer 7b   │ P/T setting effects (incl. "base P/T")                      │
│ Layer 7c   │ P/T modifying effects AND P/T counters                      │
│ Layer 7d   │ P/T switching                                               │
├────────────┴─────────────────────────────────────────────────────────────┤
│ Post-layer pass A (613.10): effects that affect PLAYERS (timestamp order) │
│ Post-layer pass B (613.11): RULE-MODIFYING effects — costs ordered per    │
│   601.2f; all others in timestamp order (with 613.8 dependency)           │
└──────────────────────────────────────────────────────────────────────────┘
```
* **There is no "Layer 8."** CR 613.1 defines only layers 1–7. Player-affecting effects (613.10) and rule-modifying effects (613.11) are applied afterward. These include maximum hand size, extra land plays, attack and block requirements and restrictions, "as though" permissions, and cost changes.
* **Prototype, Adventure, Omen and preparation characteristics, split halves and Room halves** are alternative characteristics or layout rules that pick the **base** characteristics before layer 1 begins. Layer 1b handles face-down status only.
* **Keyword counters** (CR 122.1b) are applied in layer 6. The valid keyword-counter kinds are: flying, first strike, double strike, deathtouch, decayed, exalted, haste, hexproof, indestructible, lifelink, menace, reach, shadow, trample and vigilance, plus variants of those keywords (e.g. "hexproof from"). **Ward is not a keyword-counter kind.**

### 6.2 Ordering Within a Layer (`REQ-LAYER-002`) (CR 613.3, 613.7, 613.8)
Within each layer or sublayer, the engine SHALL order effects as follows:
1. **CDAs first (613.3, 613.4a).** In layers 2–6, effects from characteristic-defining abilities apply before all other effects. Layer 7a consists *only* of CDAs. Layer 1 has no CDA-first rule. Its sublayers apply in timestamp order, adjusted by dependency (613.2).
2. **Dependency (613.8a).** Effect $B$ *depends on* effect $A$ if:
   * (a) they apply in the same layer and sublayer;
   * (b) applying $A$ would change the text or existence of $B$, what $B$ applies to, or what $B$ does to anything it applies to; **and**
   * (c) neither is from a CDA, or both are.
3. **Selection (613.8b–c).** Repeat until every effect has been applied:
   * Let $U$ be the effects not yet applied, and compute the dependency relation over $U$ against the *current* partially evaluated state.
   * **Candidates:** effects in $U$ that depend on no other effect in $U$. Apply the candidate with the earliest timestamp.
   * **Dependency loops:** if every effect in $U$ depends on another (no candidates), find a dependency loop (a strongly connected component with no incoming edges from outside it). Per 613.8b, dependency is ignored for the loop's members, and **all members of the loop are applied in timestamp order**. Apply the earliest-timestamped member, recompute, and repeat. Recomputing after each application is required by 613.8c, because applying one member can break the loop.
   * After each application, recompute the dependency relation (613.8c).
4. **Dependency checks are mechanical.** REQ-DSL-004 requires every effect to declare its layer, its affected-set selector and its modification. The engine checks condition (b) by *trial application*: apply $A$ to a scratch copy, then compare $B$'s existence, affected set and output with and without $A$. This SHALL be implemented generically, with no per-card special cases.

**Canonical examples for the test suite (REQ-TEST-001):**
* **Dependency:** *Blood Moon* + *Urborg, Tomb of Yawgmoth*. Both apply in layer 4, and Blood Moon removes Urborg's ability, so Urborg's effect depends on Blood Moon's.
* **Dependency:** *Opalescence* (a layer-4 type change) + another layer-4 effect that applies only to creatures, such as one that adds a creature type to all creatures. Opalescence changes what the second effect applies to, so the second effect depends on it.
* **Timestamp, not dependency:** *Humility* + *Opalescence*. This is the classic timestamp-order case: the final P/T outcome depends on which entered first. It SHALL be tested as a timestamp case.
* **Lock-in across layers (613.6):** *Svogthos, the Restless Tomb*, using the example in 613.6.

### 6.3 Lock-In Rules (`REQ-LAYER-003`)
* **CR 613.6.** An effect that applies in several layers applies each part in its own layer. **Once it starts applying in one layer, it keeps applying to the same set of objects in every later layer, even if the ability creating it is removed during evaluation.** The evaluator SHALL record, for each multi-layer effect, the affected set chosen in the first layer where it applies, and reuse that set in later layers.
* **CR 611.2c.** A continuous effect created by a *resolving* spell or ability that changes characteristics or control fixes its affected set when the effect begins. That set never changes afterward. The effect stores a concrete `Set<ObjectRef>`.
* **CR 611.3a.** A continuous effect from a *static* ability is not locked in. Its selector is re-evaluated every time characteristics are computed.
* **CR 611.2b.** A "for as long as" duration that never starts, or that ends before the effect would first apply, does nothing.

### 6.4 Post-Layer Passes (`REQ-LAYER-004`)
* **Pass A (613.10)** computes player properties from player-affecting effects, in timestamp order with dependency. Examples: protection, hexproof, "can't lose the game", "can't gain life", and maximum hand size when it is phrased as a player effect.
* **Pass B (613.11)** computes the **RuleSet view** used by all procedural query points (§14.6): costs, permissions, prohibitions, combat requirements and restrictions, skipped steps, extra land plays, "as though" effects, and how mana empties. Cost-affecting effects are ordered by the 601.2f procedure (§7.1). All others go in timestamp order.

### 6.5 Performance & Caching (`REQ-LAYER-005`)
* The evaluator SHALL cache evaluated characteristics, keyed by `(stateVersion, ObjectId)`. The cache SHALL be invalidated whenever anything changes that can affect an effect's existence, selector or output. That includes zone changes, control changes, counter changes, effect registration or expiry, timestamps, designations, attachment and status changes, and any change to a player value that a CDA or condition reads.
* A **full-recompute debug mode** SHALL exist. CI SHALL run the full regression suite in both cached and full-recompute modes and require identical state hashes (differential testing).
* Expected evaluation cost SHALL be documented. The target is ≤ 2 ms per full-board recompute for a 4-player Commander board with 200 permanents on reference hardware. It is a target, not a correctness requirement.

---

## 7. Casting, Activating & Resolving (CR 601–608)

### 7.1 Casting Procedure (`REQ-CAST-001`) (CR 601.2a–i; activation 602.2 follows the same steps)
| Step | CR | Engine operation |
| :--- | :--- | :--- |
| **0** | 734 | Take checkpoint $S_{\text{checkpoint}}$ (§2.5). |
| **1** | 601.2a | **Announce.** Move the card to the stack, or create the ability or copy there, with a new ObjectId. For a layout that offers a choice (split half, MDFC face, Adventure, Omen, prototype, transformed/converted cast, face-down cast), the choice is made now, and only the chosen characteristics are checked (712.8c, 716.3a, 719.3a, 721.3a, 708.4). Check casting permissions and prohibitions (613.11 RuleSet): timing, zone of origin, "can't cast" effects. |
| **2** | 601.2b | **Choices:** modes (incl. spree, escalate, entwine, tiered); at most **one alternative cost** (e.g. flashback, overload, evoke, dash, blitz, bestow, impending, freerunning, plot, warp, harmonize, mutate, prototype casting, "without paying its mana cost"); any **additional costs** (e.g. kicker, multikicker, buyback, offspring, squad, casualty, bargain, gift, waterbend-as-additional-cost, "as an additional cost, sacrifice…"); the value of X; how hybrid and Phyrexian symbols will be paid; and, for harmonize, which creature will be tapped (702.180b). |
| **3** | 601.2c | **Targets** for each target slot. A target is legal if it matches the target predicate and no prohibition applies: hexproof (702.11), shroud (702.18), protection (702.16), "can't be the target of". **Ward is NOT checked here.** It is a triggered ability (§8.2, 702.21a). |
| **4** | 601.2d | **Division:** if the spell divides an amount among targets, assign each target at least 1. |
| **5** | 601.2e | **Legality check** of the fully specified spell against the 613.11 RuleSet (e.g. "spells with mana value 4 or greater can't be cast"). If it fails, roll back (734). |
| **6** | 601.2f | **Determine and lock in the total cost.**<br>(a) Start from the mana cost, or the chosen alternative cost.<br>(b) Add all additional costs and cost increases.<br>(c) Subtract cost reductions, **in any order the player chooses**. The mana part of the cost can't go below {0}.<br>(d) **Then apply effects that directly affect the total cost**, such as Trinisphere-style minimums.<br>(e) **Lock in.** Later changes to cost effects have no effect on this spell.<br>Formally: $C = \text{floorEffects}\big(\max(\{0\}, \text{base} + \text{additional} + \text{increases} - \text{reductions}_{\text{ordered}})\big)$. |
| **7** | 601.2g | **Mana ability window** (605.3a). |
| **8** | 601.2h | **Pay the total cost.** First pay every component that involves neither randomness nor moving cards from a library to a public zone, in any order. Then pay the rest, in any order. Partial payment is not allowed. **Payment-method abilities apply here, not in step 6:** convoke (702.51), improvise (702.126), delve (702.66), assist (702.132), and waterbend (701.67, which lets you tap an artifact or creature instead of paying generic mana, limited to the waterbend portion per 701.67b). They do not change the total cost. If the cost can't be paid, roll back. |
| **9** | 601.2i | **Commit.** The spell becomes cast, or the ability becomes activated. Record spell history in Memory (§2.6). Cast/activate triggers go to the pending queue. Examples: storm (702.40), cascade (702.85), prowess (702.108), increment (702.191), and "whenever you cast". A prepared permanent loses `Prepared` when its copy becomes cast (723.3c). |

* **Activated abilities (602).** The DSL declares activation restrictions (§14.4.2): sorcery timing, "only once each turn", "only during your turn", "only if…", and zone of function. These are checked in step 1. Loyalty abilities (606) are limited to sorcery timing and once per turn per permanent, unless rules modifications say otherwise. **The {T}/{Q} summoning-sickness check (302.6)** applies to creatures only, using the continuous-control tracking from §3.1.
* **Mana abilities (605).** A mana ability is an activated ability that has no target, could add mana when it resolves, isn't a loyalty ability, and doesn't move cards to or from a library (605.1a). The same applies to triggered mana abilities (605.1b). Mana abilities don't use the stack (605.3b) and can't be activated again before they resolve (605.3c). Mana with restrictions or riders (e.g. "spend only on artifact spells", or "if spent on a creature spell, it gains haste") SHALL be tracked per mana unit in the pool.

### 7.2 Resolution (`REQ-CAST-002`) (CR 608)
1. **Target recheck (608.2b).** A target is illegal if it has left the zone it was in, or no longer meets the target requirements. If the ability's source has left its zone, its LKI is used for this check. If **every** target for **every** instance of "target" is illegal, the spell or ability doesn't resolve: it is removed from the stack and, if it is a spell, put into its owner's graveyard. Otherwise it resolves, and illegal targets are unaffected.
2. **Intervening-if recheck (603.4)** for triggered abilities: if the condition is false, the ability is removed from the stack and does nothing.
3. **Instructions (608.2c)** execute in printed order. Each atomic change is sent through the replacement/prevention bus (§8.1). Information from the game is determined once, when the effect is applied (608.2h). Choices made during resolution become `PendingChoiceRequest`s (§15.3).
4. **Leaving the stack mid-resolution (608.2m):** the spell keeps resolving.
5. **Final step (608.2n, 608.3):**
   * An instant or sorcery is put into its owner's graveyard, unless a replacement or keyword says otherwise. Examples: flashback (702.34) and harmonize (702.180a) exile it, rebound (702.88), buyback (702.27) returns it to hand, cipher (702.99), and an Adventure is exiled (716.3d).
   * **An Omen is shuffled into its owner's library** (721.3d), and **a paradigm spell is exiled** (702.192a).
   * A permanent spell becomes a permanent under its controller's control. A mutating spell with a legal target merges instead (702.140c). A copy of a permanent spell becomes a token (707.10f).

### 7.3 Copies of Spells & Abilities (`REQ-CAST-003`) (CR 707.10)
A copy of a spell or ability copies its characteristics and **all decisions made for it** (modes, targets, X, additional and alternative costs). It does not copy choices made during resolution. A copy isn't cast or activated. When an effect lets a player choose new targets for the copy (707.10c), the player may keep any original targets, even illegal ones, but any new target must be legal. "Copy for each other player or object it could target" follows 707.10d. A copy of a spell in a zone other than the stack stops existing (704.5e). Exceptions are explicit, e.g. the prepare-spell copy in exile (723.3c).

---

## 8. Event Interception: Replacement, Prevention & Triggers

### 8.1 Replacement & Prevention Bus (`REQ-EVENT-001`) (CR 614–616)
Every atomic change to the game SHALL be represented as a `GameEvent` and routed through this bus. That covers zone changes, drawing, damage, life change, counters, tap/untap, creating tokens, beginning a turn, phase or step, attaching, becoming a copy, turning face up or down, and so on. Each `GameEvent` carries an `AppliedEffects: Set<EffectId>` set (614.5).

```
Incoming GameEvent E  (affecting object/player X)
  │
  ▼ [1] Collect applicable replacement/prevention effects R(E),
  │      evaluated against the CURRENT state (or the 614.12 projected
  │      state for ETB-modifying effects), excluding E.AppliedEffects
  │      (614.5) — except where a rule overrides 614.5 (e.g. 903.9b).
  │
  ├── R(E) empty → EXECUTE E; record for trigger detection (§8.2)
  │
  ▼ [2] Chooser = controller of affected object (owner if none), or the
  │      affected player (616.1). Simultaneous choosers → APNAP.
  ▼ [3] Restrict choices by precedence tier (first non-empty tier wins):
  │      616.1a  self-replacement effects (614.15)
  │      616.1b  effects changing under whose control it enters
  │      616.1c  effects making it enter as a copy
  │      616.1d  effects making a card enter with its back face up
  │      616.1e  any other applicable effect
  ▼ [4] Chooser picks one R*; E' = R*(E); E'.AppliedEffects += R*
  ▼ [5] 616.1f: repeat from [1] on E' (newly applicable effects
  │      included, 616.2) until none remain
  └── 616.1g: an effect on an event CONTAINED in E (e.g. "enters"
         inside "create token") can't be chosen until E's own
         replacement has been chosen.
```
* **ETB look-ahead (614.12).** To decide which replacement effects apply to a permanent entering the battlefield, the engine SHALL build a **projected permanent**: its characteristics as they would be on the battlefield, taking into account (a) replacement effects already applied to this entry, (b) continuous effects from its own static abilities, and (c) continuous effects already in force. Choices these effects require are made before it enters (614.12a). The total cost of simultaneous choices must remain payable (614.12b). Anchor-word choices are recorded in Memory (614.12c).
* **Prevention (615).** Prevention shields, "prevent the next N" counters and "prevent all" effects are bus effects that consume their shields. Shield counters (122.1c) create one replacement effect (destroy) and one prevention effect (damage).
* **Same-result replacement merging (704.7).** If several SBAs would produce the same result at the same time, a single replacement effect replaces all of them.
* **Replacement effects that create events** (e.g. "instead, exile it and return it…") produce child events that go through the bus themselves.

### 8.2 Triggered Abilities (`REQ-EVENT-002`) (CR 603)
After each event, or each batch of simultaneous events, completes:
1. **Normal detection (603.2, 603.10).** Match every active trigger's event matcher (§14.4.1) against the event, using the **post-event state**: objects and abilities as they exist immediately after the event.
2. **Look-back detection (603.10a–g).** For triggers in the categories below, match against the **pre-event state**, using the abilities that existed and how objects looked immediately before the event. The categories are fixed by the CR and SHALL be derived automatically from the event matcher's kind:
   * **603.10a:** leaves-the-battlefield abilities; abilities that trigger when a player **sacrifices** a permanent; abilities that trigger when a card **leaves a graveyard**; abilities that trigger when an object all players can see is put into a **hand or library**;
   * **603.10b:** a permanent phasing out;
   * **603.10c:** an object specifically becoming unattached;
   * **603.10d:** a player losing control of an object, or an opponent gaining control of it from that player;
   * **603.10e:** a spell being countered;
   * **603.10f:** a player losing the game;
   * **603.10g:** a player planeswalking away from a plane.

   Crew, saddle and station abilities are *not* look-back triggers. Abilities that care about the *characteristics of tapped creatures* use the cost-payment LKI recorded in the ability's bindings (400.7j).
3. **Intervening-if (603.4).** The condition is checked when the event happens. If it is false, the ability doesn't trigger. If it triggers, the condition is checked again on resolution (§7.2).
4. **State triggers (603.8).** A state trigger triggers as soon as the game state matches its condition. It doesn't trigger again until the existing instance has resolved, been countered or otherwise left the stack, at which point it re-checks.
5. **Delayed triggers (603.7)** are registered by effects and capture their bindings at creation. **Reflexive triggers (603.12)** ("when you do") are created during resolution and trigger right away if the action was taken.
6. **Inherent sourceless triggers.** Monarch (726.2), Initiative (727.2), rad counters (729.1) and speed (702.179d) are triggers controlled by a player with no source object (an exception to 113.8). They SHALL be modeled as player-scoped trigger registrations.
7. **Putting triggers on the stack (603.3b–d).** Waiting triggers go on the stack the next time a player would receive priority, in APNAP order. Each player chooses the order of their own triggers. Modes and targets are chosen as each trigger is put on the stack. A trigger with no legal targets is removed.
8. **"Triggers only once each turn"** and similar limits are tracked per ability instance in Memory.

---

## 9. State-Based Actions (CR 704)

`REQ-SBA-001`: In step 1 of the heartbeat, the engine SHALL evaluate the entire state and perform **every** applicable SBA simultaneously as one event (704.3). The table follows the Rules Baseline's lettering.

| Rule | Condition | Result |
| :--- | :--- | :--- |
| **704.5a** | A player has 0 or less life. | That player loses. |
| **704.5b** | A player attempted to draw from an empty library since the last SBA check. | That player loses. |
| **704.5c** | A player has 10 or more poison counters (ignored in Two-Headed Giant; see 704.6b). | That player loses. |
| **704.5d** | A token is in a zone other than the battlefield. | It ceases to exist. |
| **704.5e** | A copy of a spell is in a zone other than the stack, or a copy of a card is in a zone other than the stack or battlefield (exception: 723.3c prepare copies). | It ceases to exist. |
| **704.5f** | A creature has toughness 0 or less. | It is put into its owner's graveyard. Regeneration can't replace this. |
| **704.5g** | A creature with toughness > 0 has damage marked on it ≥ its toughness. | It is destroyed. Regeneration and shield counters can replace this. |
| **704.5h** | A creature with toughness > 0 was dealt damage by a deathtouch source since the last check. | It is destroyed. |
| **704.5i** | A planeswalker has loyalty 0. | It is put into its owner's graveyard. |
| **704.5j** | **Legend rule:** a player controls two or more legendary permanents with the same name. | That player chooses one. The rest are put into their owners' graveyards. |
| **704.5k** | **World rule:** two or more permanents have the world supertype. | All except the one that has had the world supertype for the shortest time are put into graveyards. If there is a tie, all of them are. |
| **704.5m** | An Aura is attached illegally, or not attached. | It is put into its owner's graveyard. |
| **704.5n** | An Equipment or Fortification is attached to an illegal permanent or to a player. | It becomes unattached and stays on the battlefield. |
| **704.5p** | A battle or creature is attached to something, or a non-Aura/Equipment/Fortification noncreature, nonbattle permanent is attached. | It becomes unattached. |
| **704.5q** | A permanent has both +1/+1 and −1/−1 counters. | Remove N of each, where N is the smaller count. |
| **704.5r** | A permanent has more counters of a kind than an ability allows. | Remove the excess. |
| **704.5s** | A Saga's lore counters ≥ its final chapter number, and none of its chapter abilities is on the stack or waiting. | Its controller sacrifices it. |
| **704.5t** | A venture marker is on a dungeon's bottommost room, and no room ability of that dungeon is on the stack or waiting. | The dungeon's owner removes it from the game. |
| **704.5u** | A space sculptor permanent is on the battlefield along with creatures that have no sector. | Sector designations are chosen in the 704.5u order. |
| **704.5v** | A Siege battle has defense 0, and none of its triggered abilities is on the stack or waiting. | It is put into its owner's graveyard. |
| **704.5w** | A non-Siege battle has defense 0. | It is put into its owner's graveyard. |
| **704.5x** | A battle has no protector and is not being attacked. | Its controller chooses an appropriate protector. If none can be chosen, it is put into the graveyard. |
| **704.5y** | A battle's protector can't be its protector. | Its controller chooses a new protector. If none can be chosen, it is put into the graveyard. |
| **704.5z** | **Role rule:** a permanent has more than one Role attached that the same player controls. | All of that player's Roles on it except the most recent are put into their owners' graveyards. |
| **704.5aa** | A player controls a permanent with *start your engines!* and has no speed. | That player's speed becomes 1. |
| **704.6a** | Two-Headed Giant: a team has 0 or less life. | That team loses. |
| **704.6b** | Two-Headed Giant: a team has 15 or more poison counters. | That team loses. |
| **704.6c** | Commander: a player has been dealt ≥ 21 combat damage by the same commander over the game. | That player loses. |
| **704.6d** | Commander: a commander was put into a graveyard or exile since the last check. | Its owner may put it into the command zone. |
| **704.6e** | Archenemy: a non-ongoing scheme is face up in the command zone, and none of its triggers is on the stack or waiting. | The scheme is turned face down and put on the bottom of the scheme deck. |
| **704.6f** | Planechase: a phenomenon is face up, and none of its triggers is on the stack or waiting. | The planar controller planeswalks. |

* **Data-driven SBA registry.** SBAs SHALL be implemented as entries in an internal registry (`id`, `crRef`, `predicate`, `action`, `variantGate`). That way a future CR release that adds an SBA is a localized change, and the citation CI (REQ-DOC-001) can map each registry entry to its rule.

---

## 10. Predefined Tokens, Counters & Designations

### 10.1 Predefined Tokens (`REQ-SUB-001`) (CR 111.10)
The engine SHALL include built-in definitions for every predefined token in CR 111.10, because these are rules text, not card IP. As of the Rules Baseline they are:
* Treasure (a), Food (b), Gold (c), Walker (d), Shard (e), Clue (f), Blood (g), Powerstone (h), Incubator (i; double-faced: an Incubator artifact that transforms into a 0/0 Phyrexian artifact creature);
* the Roles: Cursed (j), Monster (k), Royal (m), Sorcerer (n), Virtuous (p), Wicked (q), Young Hero (r);
* Map (s), Junk (t), Lander (u), Mutagen (v), Vibranium (w), Heartwood (x; a red and green artifact with "{T}: Add {R} or {G}").

The predefined-token table SHALL be **data**, loaded from the `RulesVocabulary` artifact and generated from the CR text by the toolchain. That way new CR releases add tokens without engine changes. The creating effect may modify or add to the predefined characteristics (111.10). The Empower Jace token (701.71a) is defined by its keyword action, not by 111.10.

### 10.2 Counters (`REQ-SUB-002`) (CR 122)
* **Keyword counters (122.1b):** see §6.1 for the 15 kinds. Applied in layer 6.
* **Shield counters (122.1c):** one replacement effect ("if this would be destroyed as the result of an effect, instead remove a shield counter") and one prevention effect ("if damage would be dealt to this, prevent it and remove a shield counter").
* **Stun counters (122.1d):** "if this would become untapped, instead remove a stun counter."
* **Finality counters (122.1h):** "if this would be put into a graveyard from the battlefield, exile it instead."
* **Player counters:** poison, energy, experience, rad (729: an inherent precombat-main trigger that mills N, then for each nonland card milled the player loses 1 life and removes a rad counter), and ticket counters.
* **Counter kinds are open-ended.** Any string kind is allowed. Only the kinds above carry built-in rules.

### 10.3 Game & Player Designations (`REQ-SUB-003`)
1. **The Monarch (CR 726):** `MonarchPlayerId?`. Two inherent sourceless triggers (726.2). Only one monarch at a time (726.3). Handling when the monarch leaves the game: 726.4.
2. **The Initiative (CR 727) and dungeons (CR 309, 701.49):** per-player dungeon card and room pointer. Inherent triggers for the initiative (727.2). Retaking the initiative fires the retake trigger (727.5). Dungeon cards are rules data in the card database, not engine IP.
3. **Day/Night (CR 732, 702.145):** a game designation that is `Neither`, `Day` or `Night`. Once set, the game is always Day or Night (732.1). The untap-step check is 502.2. Daybound and nightbound permanents transform through their static abilities when the designation changes.
4. **The Ring Tempts You (CR 701.54):** a per-player count of how many times the Ring has tempted them (abilities unlock at 1, 2, 3 and 4 or more), plus a Ring-bearer designation on a creature.
5. **Speed (CR 702.178–179):** per player, `null` or 1–4. The SBA 704.5aa sets it to 1. Inherent trigger: "whenever one or more opponents lose life during your turn, if your speed is less than 4, it increases by 1; triggers only once each turn" (702.179d). Max speed means speed 4 (702.179e). A player with no speed counts as 0 for effects that read speed (702.179f).
6. **City's blessing / ascend (CR 702.131):** ascend on an instant or sorcery is a **spell ability** checked on resolution (a). Ascend on a permanent is a **static ability** checked continuously (b). After a player gets the blessing, continuous effects are reapplied before trigger detection (d).
7. **Permanent designations** (§3.1): unlocked halves, prepared, goaded, monstrous, renowned, suspected, saddled, solved, class level, sector, and others. This is an open registry.

---

## 11. Formats, Multiplayer & Extensions (CR 100–104, 800–905)

### 11.1 Format Configuration (`REQ-FMT-001`)
Every match starts from a `FormatConfig`. Rules that the CR leaves to the format, or to the casual variant, SHALL be configurable here and SHALL NOT be hardcoded.
```json
{
  "formatId": "commander",
  "rulesBaseline": "CR-2026-11-06",
  "deck": { "minMain": 100, "maxMain": 100, "maxSideboard": 0, "maxCopies": 1,
            "basicLandExempt": true, "enforceColorIdentity": true },
  "players": { "min": 2, "max": 6, "teams": null, "sharedTeamTurns": false,
               "rangeOfInfluence": null, "attackMultiplePlayers": true },
  "startingLife": 40,
  "startingHandSize": 7,
  "mulligan": { "rule": "LONDON", "freeFirstMulliganMultiplayer": true },
  "firstTurnDrawRule": "CR_DEFAULT",
  "commander": { "enabled": true, "damageThreshold": 21, "taxPerCast": 2,
                 "removePerpetualOnCommandZone": true },
  "cardPolicy": { "minSupportStatus": "AUTO_PARSED", "onPartial": "WARN",
                  "onUnsupported": "REJECT", "allowDigitalOnly": false },
  "timeoutPolicy": "AUTO_PASS_THEN_DEFAULT_CHOICE",
  "deckListsOpen": false,
  "ante": false,
  "digitalExtensions": { "enabled": false, "set": [] },
  "banlistRef": "banlists/commander@2026-11-10"
}
```
* `firstTurnDrawRule`: `CR_DEFAULT` follows 103.8a–c. Other values exist for house rules.
* `cardPolicy` connects the format to card coverage (REQ-ING-008). Sanctioned or tournament modes SHOULD require `VERIFIED`.
* Ban lists and legalities are versioned data, never engine code.

### 11.2 Commander (`REQ-FMT-002`) (CR 903)
* **Eligible commanders (903.3).** A legendary card that is (a) a creature card, (b) a Vehicle card, or (c) a Spacecraft card with one or more power/toughness boxes. Cards with "can be your commander" also qualify (903.3a). Commander status is an **attribute of the card** that survives zone changes, being turned face down and copying. A permanent that copies a commander is *not* a commander (903.3). Special cases: a melded or merged permanent that contains the commander is the commander (903.3b–c).
* **Partner abilities (702.124).** Partner; partner—[text] (the variants include Character select, Father & son, Friends forever and Survivors); partner with [name]; choose a Background; and Doctor's companion. Different partner abilities can't be combined (702.124f). A player can never have more than two commanders (702.124g).
* **Color identity (903.4)** comes from each card's color-identity attribute. With partners, it is the union of both commanders.
* **Commander tax (903.8):** {2} more for each previous cast **from the command zone**, counted per commander card (702.124d).
* **Returning to the command zone.**
  * From graveyard or exile: an SBA, where the owner may choose (903.9a, 704.6d).
  * From hand or library: a replacement effect (903.9b) that **may apply more than once to the same event, an exception to 614.5**. The bus (§8.1) SHALL support this per-effect exception flag.
  * For a melded or merged commander, only the commander card goes to the command zone (903.9c).
* **Commander damage (903.10a, 704.6c):** `CombatDamageDealt[PlayerId][CommanderCardId]`, counting only combat damage dealt by that card while it is a commander. Each partner commander is tracked separately (702.124d).
* **Brawl (903.12 family)** is a FormatConfig preset.

### 11.3 Community Formats (`REQ-FMT-005`)
Formats that are not defined in the CR SHALL be implemented as FormatConfig presets, plus **rules-extension modules** where needed, and never as changes to core rules. They SHALL be labelled `nonCR: true`. **Oathbreaker** is one such format. It has one planeswalker as the Oathbreaker and one instant or sorcery as the Signature Spell, both in the command zone. The Signature Spell can be cast only while its owner controls their Oathbreaker, it returns to the command zone instead of changing zones, and it has its own tax. Its rules come from the format's own published rules, not from the CR, and its rules module SHALL cite those rules.

### 11.4 Multiplayer: Leaving the Game (`REQ-FMT-003`) (CR 800.4)
When a player leaves the game (loses, concedes or draws out) and the game continues (800.4):
1. **(800.4a)** All objects that player **owns** leave the game, and every effect that gives that player control of objects or players ends. Then any objects on the stack that player controls and that aren't represented by cards cease to exist. Then any objects that player still controls are **exiled**. This is not an SBA; it happens immediately. If the player had priority, priority passes to the next player in turn order.
2. **(800.4b–d)** Control changes, token creation, entering under that player's control, and triggers they would control are all prevented. When a control effect ends and the default controller has left, the object is exiled (800.4c).
3. **(800.4e–h)** Combat damage that would be assigned to them isn't assigned. Costs they would pay aren't paid. Choices they would make are reassigned as those rules describe.
4. **(800.4i)** Effects that need information about them use their last known information.
5. **(800.4j–k)** If they leave during their own turn, the turn continues **without an active player**: priority goes to the next player in turn order, or the top object resolves, or the step ends. Their future turns don't begin.
6. **(800.4m)** Effects lasting "until that player's next turn" last until that turn would have begun.
7. **(800.4n)** Objects they own in the ante zone stay there. **(800.4p)** If they were the planar controller, that role passes to the next player.
8. Monarch and Initiative pass as described in 726.4 and 727.4.

### 11.5 Digital-Only Mechanics Extension (`REQ-FMT-004`)
Alchemy and other designed-for-digital mechanics (conjure, perpetual, seek, spellbook, specialize, double team, draft, heist, incorporate, intensity, boon and others) are **not part of the Comprehensive Rules**. MTG Arena's programmed behavior defines them. The engine SHALL therefore implement them as an **optional extension module** (`ext.digital`), enabled per FormatConfig, with these rules:
* Every extension primitive SHALL be documented in a separate `EXT-DIGITAL` specification. It records the source of each behavior (Arena release notes and observed Arena behavior) and is versioned separately from the CR baseline.
* **Conjure:** creates a new `CardId` (a real card, not a token, so 704.5d does not apply) owned by the conjuring player, in the stated zone. A conjured duplicate does **not** inherit perpetual modifications from the original.
* **Perpetual:** a modification attached to the `CardId` that persists through zone changes for the rest of the game. **Design decision (not a CR rule):** perpetual modifications change the card's *base* characteristics before layer 1 begins. They are therefore copiable only when the copy effect copies a card with those modifications, and they are visible in every zone. As a Commander-variant convention, a commander's owner may remove all perpetual effects from it as it moves to the command zone (`removePerpetualOnCommandZone`).
* **Seek:** puts a random library card matching a predicate into hand, using the PRNG. It does not reveal the card or shuffle the library. Position knowledge (§2.4) of the library is invalidated only for the index that was removed and the indices below it.
* **Spellbook/Draft:** choose from a random sample of a named card pool; the pool is data.
* **Specialize:** a permanent, irreversible change of the card's definition reference to the variant for the chosen color or class, for the rest of the game.
* A deck that contains digital-only cards SHALL fail validation in any FormatConfig where `digitalExtensions.enabled == false`.

---

## 12. Loops, Shortcuts & Verification

### 12.1 Loop Handling (`REQ-LOOP-001`) (CR 104.4b, 733)
1. **Canonical state hash.** After each heartbeat iteration the engine SHALL compute a canonical hash $H(S_t)$ of the **rules-relevant** state. That state covers zones and their order, evaluated characteristics, counters, designations, the stack, pending triggers, players' life and counters, the turn, step and priority position, and the PRNG state. It excludes monotonic bookkeeping that never affects game outcomes, such as replay indices and the timestamp counter, but includes *relative* timestamp order. The hash SHALL be incremental (Zobrist or Merkle).
2. **Mandatory loops (104.4b, 733.4).** While the engine is advancing with **no choices pending for any player**, it SHALL record the hashes it visits. If a hash repeats with no player choice in between, the loop contains only mandatory actions and the game is a **draw**. (With the limited range of influence option, the 104.4b variant applies.)
3. **Choices that don't break the loop.** A choice whose only legal option, for every possible answer, leads to the same repeating cycle counts as mandatory. Per 733.5, players can't be forced to use unrelated abilities to break a loop. Per 733.6, "[A] unless [B]" counts as mandatory if no player chooses B.
4. **Optional loops (733.2–733.3).** The engine SHALL implement the shortcut protocol:
   * The player with priority may propose a shortcut: a sequence of game choices for all players, including "repeat N times", with no conditional actions (733.2a).
   * Each other player, in turn order, either accepts or **shortens** the sequence by naming the point where they will make a different choice (733.2b).
   * The engine then advances to the agreed end point. If the sequence was shortened, the player who now has priority must make a different choice (733.2c).
   * **Fragmented loops (733.3):** if different players' independent actions reach the same state repeatedly, the active player (or the first player in turn order who is involved) must make a different choice.
   * **Fast-forwarding.** The engine SHALL fast-forward a loop either by applying the per-iteration state delta N times in closed form, when the delta is linear in counters, life, tokens and similar values, or by iterating with a time limit.
5. **Safety valve.** If the number of automated (choice-free) steps exceeds `K_max` (default $10^6$) without a repeated hash, the engine SHALL raise an `EngineFaultEvent{ kind: "STEP_BUDGET_EXCEEDED" }` and pause for the host to decide. This SHALL NOT be treated as a game result. Long finite chains, such as large storm counts or token cascades, are legal and must be allowed to finish.
6. **Tournament overlay.** Tournament Rules take precedence during tournaments (733.1c). Tournament loop policy SHALL be a separate configuration layer.

### 12.2 Test Harness & Conformance (`REQ-TEST-001`)
1. **Puzzle format (`.pzl`).** A declarative scenario file containing: format, rules baseline, the card database version and the definition hashes it requires, players, zones with exact contents and order, statuses, counters, designations, the turn, phase and step, priority, the PRNG seed, a scripted sequence of player choices, and **assertions**. Assertions can check evaluated characteristics, zone contents, life, the event stream (a subsequence or exact sequence), pending choices, and game results.
2. **Rules regression suite.** The suite SHALL use **synthetic test cards**: invented card definitions that reproduce the abilities of the cards named below, with no real card names or Oracle text (REQ-NFR-004). A named card below identifies the interaction to reproduce, not a dependency on that card's data. At minimum it SHALL contain:
   * **Layers:** *Humility* + *Opalescence* in both timestamp orders; *Blood Moon* + *Urborg* + *Dryad of the Ilysian Grove*; the *Svogthos* example from 613.6; a CDA-in-layer-4 case (e.g. changeling with a type-setting effect); a Layer 3 text-change case (*Mind Bend* style); 613.7m simultaneous timestamps.
   * **Replacement:** *Rest in Peace* + *Blood Artist* + board wipe (replacement plus look-back); a 616.1 tier test (copy ETB versus control ETB); the 616.1g nested event (*Doubling Season* + "as enters, choose"); 903.9b repeated application; *Sylvan Library* + *Abundance*/dredge.
   * **Casting:** *Panglacial Wurm* + *Selvala, Explorer Returned* (734.1 partial reversal); a 601.2f reduction-ordering test plus a Trinisphere minimum; payment with convoke, delve and waterbend; ward triggering and its optional payment; harmonize creature tapping.
   * **Combat:** division among multiple blockers with no damage-assignment-order prompt (510.1c); trample with lethal floors that account for other assignments; trample over planeswalkers; first strike and double strike steps; a creature gaining double strike between the two damage steps; banding; menace with requirements; Propaganda-style attack costs; goad in multiplayer.
   * **Layouts:** Omen shuffled into the library; Adventure exiled then cast from exile; prepare copy creation and control-change behavior (723.3c); Station thresholds and the becoming-a-creature P/T box; Room unlock special action and "fully unlock" triggers; meld MV and leaving the battlefield; mutate over and under with a face-down component.
   * **SBAs:** every entry in §9, each with a dedicated test.
   * **Multiplayer:** leaving the game with every 800.4 subrule; monarch and initiative passing.
   * **Loops:** a mandatory loop ending in a draw; an optional loop shortcut that an opponent shortens; a long finite chain that does *not* trip the draw rule.
3. **Card-level tests.** See REQ-ING-010.
4. **Differential testing.** The cached and full-recompute layer modes (REQ-LAYER-005) SHALL be compared, and so SHALL replays across all language bindings (REQ-CORE-006).
5. **Fuzzing.** A random-legal-action fuzzer SHALL run nightly over the synthetic test-card library (and MAY run over a client-built card database where one is available to the CI cache, REQ-NFR-004), checking invariants: no exceptions; the state hash is deterministic on replay; the redacted view never leaks hidden information (checked by comparing it to the omniscient state); every legal action offered by `getPendingChoice` is accepted by `submitAction`.

---

## 13. Public Engine API & Sandbox Interface

### 13.1 Embeddable Architecture (`REQ-API-001`)
 The engine SHALL be built as a dependency-free core library: a Rust core (REQ-NFR-001) with a C-ABI FFI, a .NET binding and a WebAssembly module, as specified in REQ-NFR-002. It SHALL run in-process or on a dedicated server.
* **No internal I/O.** The engine is a **synchronous state-transition function**. The host calls `submitAction()`. The engine then advances through all deterministic turn-based actions, SBAs, triggers and resolutions until it needs a player decision or the game ends, and returns a `StepResult` containing the events emitted and the next `PendingChoiceRequest`.
* **Clocks are host-side.** The engine has no notion of time. The host enforces time limits by submitting `TIMEOUT` actions (REQ-API-002).

### 13.2 Core Interface (`REQ-API-002`)
```typescript
// namespace FinePrint (REQ-ARCH-003)
export interface IRulesEngine {
  // ---------- 0. ENGINE INFO ----------
  /** Engine semver, rules baseline ID, DSL schema version, PRNG algorithm ID, enabled extensions. */
  getEngineInfo(): EngineInfo;

  // ---------- 1. LIFECYCLE ----------
  /** Loads a compiled CardDatabase artifact (§16). Returns per-card validation results and coverage stats. */
  registerCardDatabase(db: CardDatabaseArtifact): RegistrationReport;
  /**
   * Client-supplied card definitions for cards the database marks UNSUPPORTED or PARTIAL,
   * or replacing a parsed definition (REQ-ING-007). Each is validated (REQ-ING-009) and
   * smoke-tested (REQ-ING-010a) before it is accepted; optional behavioral tests may raise it to
   * VERIFIED (REQ-ING-008). Returns per-override results and the updated support report.
   */
  registerCardOverrides(overrides: CardOverrideSubmission[]): OverrideRegistrationReport;
  /** Cards that need an override, with status, knownGaps and parser residue (REQ-ING-016). */
  getCardSupportReport(filter?: CardSupportFilter): CardSupportReport;
  /** Size, copies, color identity, banlist, partner validity, AND card support policy (REQ-ING-008). */
  validateDeck(deck: DeckSubmission, format: FormatConfig): DeckValidationResult;
  createMatch(config: MatchConfig, seed: uint64): MatchSessionId;
  /**
   * Deep copy of S_t. If `determinizeFor` is set, hidden information unknown to that
   * player is resampled per REQ-API-004 and the PRNG is reseeded from (parentSeed, forkSalt).
   */
  forkMatch(sessionId: MatchSessionId, opts?: { determinizeFor?: PlayerId; forkSalt?: uint64 }): MatchSessionId;
  serializeState(sessionId: MatchSessionId): Uint8Array;
  deserializeState(payload: Uint8Array): MatchSessionId;
  exportReplayLog(sessionId: MatchSessionId): ReplayLog;
  loadReplayLog(replay: ReplayLog, upToActionIndex?: number): ReplayLoadResult; // includes drift report

  // ---------- 2. INSPECTION ----------
  getOmniscientState(sessionId: MatchSessionId): OmniscientGameState;
  getPlayerView(sessionId: MatchSessionId, viewer: PlayerId | "SPECTATOR"): RedactedGameStateView;
  getPendingChoice(sessionId: MatchSessionId): PendingChoiceRequest | null;
  /** Evaluated characteristics with per-layer contribution breakdown, dependency/timestamp order used, and RuleSet modifiers touching this object. */
  inspectObject(sessionId: MatchSessionId, objectId: ObjectId, viewer?: PlayerId): EvaluatedObjectBreakdown;
  /** Explains why an action is legal/illegal (which prohibition/requirement/permission applied). */
  explainLegality(sessionId: MatchSessionId, playerId: PlayerId, action: PlayerAction): LegalityExplanation;
  previewAutoTap(sessionId: MatchSessionId, playerId: PlayerId, cost: ManaCostExpression): AutoTapSolution[];

  // ---------- 3. ACTIONS ----------
  submitAction(sessionId: MatchSessionId, playerId: PlayerId, action: PlayerAction): EngineStepResult;
  /** Legal at any time per CR 104.3a; processed per 800.4 in multiplayer. */
  concede(sessionId: MatchSessionId, playerId: PlayerId): EngineStepResult;
  /** Host clock expired: applies FormatConfig.timeoutPolicy (auto-pass, default choice, or game loss). */
  submitTimeout(sessionId: MatchSessionId, playerId: PlayerId): EngineStepResult;
  setPlayerYields(sessionId: MatchSessionId, playerId: PlayerId, yields: PriorityYieldConfig): void;
  /** CR 733 shortcut protocol (§12.1). */
  proposeShortcut(sessionId: MatchSessionId, playerId: PlayerId, proposal: ShortcutProposal): EngineStepResult;

  // ---------- 4. SANDBOX ----------
  executeSandboxCommand(sessionId: MatchSessionId, command: SandboxCommand): EngineStepResult;
}
```

### 13.3 Sandbox Commands (`REQ-API-003`)
`executeSandboxCommand` SHALL support the following, each with `runSbaAndTriggersAfter: boolean`. A match that has used any sandbox command SHALL be flagged `sandboxed: true` in its replay header and state view.
1. `SpawnCard`: create any card definition in any zone and position, under a given owner and controller, face up or face down.
2. `MoveObject`: optionally bypassing replacement effects and triggers.
3. `MutatePlayer`: life, player counters, mana pool (including restricted mana), designations (monarch, initiative, speed, city's blessing, Ring level, day/night), lands played this turn.
4. `MutatePermanent`: status, counters, damage, designations, attachments, transform/convert, meld, merge, Memory fields.
5. `JumpToPhaseStep`: optionally clearing the stack and combat.
6. `InjectContinuousEffect` / `InjectReplacementEffect` / `InjectTrigger`: ad hoc DSL nodes.
7. `RewindToSnapshot`: undo N decision ticks.
8. `SetLibraryOrder` / `RevealTo`: set hidden-information state explicitly.

### 13.4 Determinized Forking (`REQ-API-004`)
For AI search, `forkMatch` with `determinizeFor = P` SHALL produce a state in which:
* every piece of hidden information **P does not know** (opponents' hands, unknown library positions, unknown face-down identities) is replaced by a sample that is **consistent with everything P has observed**: the identity and position knowledge in §2.4, cards revealed and still tracked, known deck contents (when FormatConfig marks deck lists as open), and cards known to have left the hidden set;
* everything P knows is preserved exactly;
* the PRNG is reseeded deterministically with `SplitMix64(parentPrngState ⊕ forkSalt)`, so repeated forks with the same salt are reproducible;
* the sampling procedure is defined (rejection sampling over the constrained multiset, using the forked PRNG) and documented. Without public deck lists, the prior over unknown cards is supplied by the host as an `UnknownCardPrior`.

### 13.5 Replay Integrity (`REQ-API-005`)
A `ReplayLog` header SHALL contain: the engine semver, the rules baseline ID, the DSL schema version, the PRNG algorithm ID, the FormatConfig (with its hash), and a `{ cardDefId → definitionHash }` map for **every card definition instantiated** in the match, including conjured cards and tokens defined by the database. `loadReplayLog` SHALL compare these with the loaded database. On any mismatch it SHALL return a `drift` report and refuse strict replay, unless the caller loads the exact pinned database version. Card databases SHALL therefore be content-addressed and retained.

---

## 14. Declarative Card DSL & AST Specification

### 14.1 Design Goals (`REQ-DSL-001`)
1. **Cards are data.** Every card is a serializable AST (canonical JSON, with MessagePack for transport) that the engine's interpreter evaluates. Cards are never imperative classes with arbitrary callbacks.
2. **The engine owns every rule; cards only contribute effects at query points.** The combat solver, priority loop, layer evaluator, cost calculator, SBA checker, and the trigger and replacement buses are generic. A card affects them **only** by adding nodes those procedures already consult (§14.6). This is what lets interactions nobody anticipated work: the declare-blockers solver never needs to know *which* card made a creature unable to block.
3. **Expressiveness target.** The schema SHALL be able to express every ability in the Oracle text of every card legal in any sanctioned format. Cards that need constructs outside the schema use the deterministic native-effect escape hatch (§14.8). The escape hatch is the measured exception, and its usage SHALL be tracked in coverage reports (REQ-ING-013).
4. **Analyzability (`REQ-DSL-004`).** Every static effect declares its layer or RuleSet kind, its selector and its modification, so dependency analysis, UI explanations and `explainLegality` work without black-box heuristics.
5. **Versioning.** The DSL schema has a semver (`dslVersion`). The toolchain migrates definitions across versions. The engine SHALL reject definitions whose major version doesn't match its own.

### 14.2 Card Definition Schema (`REQ-DSL-002`)
```typescript
export interface CardDefinitionDSL {
  cardDefId: string;                // = Scryfall oracle_id (stable across printings)
  dslVersion: string;               // e.g. "1.0"
  definitionHash: string;           // sha256 of canonical JSON of this definition
  source: {                         // provenance (REQ-ING-007/012)
    oracleTextHash: string;         // hash of normalized Oracle text + characteristics it was written against
    origin: "PARSER" | "CLIENT_OVERRIDE" | "MIXED";
    parserVersion?: string;
    overrideSource?: string;        // client-supplied identifier of who provided the override
    verifiedBy?: string[];          // definition hashes of the .pzl tests that earned VERIFIED (REQ-ING-008)
    rulesBaseline: string;          // CR version this definition was verified against
  };
  supportStatus: "VERIFIED" | "AUTO_PARSED" | "PARTIAL" | "UNSUPPORTED";
  knownGaps?: string[];             // human-readable list of unimplemented clauses (PARTIAL)
  layout: LayoutBehavior;           // §4.2 enum
  faces: CardFaceDSL[];             // ordered per layout (front/back, left/right, main/inset)
  meldPartner?: string;             // cardDefId of meld partner + result face ref
  colorIdentity: ColorBitmask;      // CR 903.4 (precomputed, validated by toolchain)
  commanderEligibility?: { eligible: boolean; partnerAbilities: PartnerAbilityDSL[] };
  legalities: Record<string, "LEGAL" | "BANNED" | "RESTRICTED" | "NOT_LEGAL">;
  digitalOnly: boolean;
  relatedTokens?: string[];         // tokenDefIds this card can create (from all_parts + parser)
}

export interface CardFaceDSL {
  faceKind: "MAIN" | "BACK" | "LEFT_HALF" | "RIGHT_HALF" | "ADVENTURE" | "OMEN"
          | "PREPARE_SPELL" | "PROTOTYPE" | "FLIPPED" | "MELD_RESULT";
  name: string;
  manaCost?: ManaCostDSL;           // structured; absent = "no mana cost"
  colorIndicator?: ColorBitmask;
  supertypes: string[]; cardTypes: string[]; subtypes: string[];
  power?: ValueExpressionAST; toughness?: ValueExpressionAST;   // absent for none
  loyalty?: number; defense?: number;
  handModifier?: number; lifeModifier?: number;
  text: TextTokenStream;            // §14.5 – the Layer-3-rewritable rules text
  abilities: AbilityAST[];          // §14.3; each references spans of `text`
}
```

### 14.3 Ability Envelope (`REQ-DSL-005`)
Every ability shares one envelope:
```typescript
type AbilityAST = SpellAbilityAST | ActivatedAbilityAST | TriggeredAbilityAST
                | StaticAbilityAST | ReplacementAbilityAST | KeywordInstanceAST;

interface AbilityBase {
  abilityId: string;                // stable within the definition, e.g. "a2"
  kind: "SPELL" | "ACTIVATED" | "TRIGGERED" | "STATIC" | "REPLACEMENT" | "KEYWORD";
  functionsIn: Zone[];              // CR 113.6 – default BATTLEFIELD for permanents, STACK for spell abilities
  textSpan?: [number, number];      // tokens in face.text it was compiled from (Layer 3 + UI)
  linkIds?: string[];               // CR 607 linked pairs (§2.6)
  abilityWord?: string;             // e.g. "Landfall" (no rules meaning, 207.2c) – for "landfall abilities" references
  condition?: PredicateAST;         // "as long as …" gating (static) or "activate only if …"
  isManaAbility?: boolean;          // validated against CR 605.1 by the toolchain
}
```

### 14.4 AST Node Families (`REQ-DSL-003`)

#### 14.4.0 References, Selectors, Predicates & Values
* **References** name players or objects relative to the ability:
  * `SELF`, `SOURCE_LKI`, `CONTROLLER`, `OWNER`, `ACTIVE_PLAYER`, `DEFENDING_PLAYER`;
  * `TARGET(n)`, `CHOSEN(key)`, `BOUND($var)`, `LINKED(linkId)`;
  * `ENCHANTED`, `EQUIPPED`, `ATTACHED_TO`, `REMEMBERED(key)`, `THAT_PLAYER`, `EACH_OPPONENT`.
* **Selectors** (`ObjectSelectorAST`) pick objects: zones plus a predicate tree (`AND`/`OR`/`NOT`), with an optional `count` and `ordering`.
* **Predicates** include card type, subtype and supertype tests, color, controller and owner, mana value comparisons, P/T comparisons, the face-up, tapped, attacking, blocking and token status flags, designations, named counters, "entered this turn", "was cast", "has ability [keyword]", and `TEXT_TOKEN_MATCH` (§14.5).
* **Value expressions** (`ValueExpressionAST`): `LITERAL`, `X`, `COUNT(selector)`, `ATTR(ref, attr)`, `PLAYER_ATTR(ref, attr)` (life, speed, hand size, devotion…), `MATH(op, lhs, rhs)` with `ADD|SUB|MUL|DIV_FLOOR|DIV_CEIL|MIN|MAX`, `DISTINCT(attr, selector)` (e.g. card types in graveyards, for Tarmogoyf and delirium), `HISTORY(query)` (e.g. "spells cast this turn", "life lost this turn", "mana spent to cast it"), and `MEMORY(key)`.
* **Integer only.** Value expressions are integer-only (REQ-CORE-006).

#### 14.4.1 Triggered Abilities — Event Matchers (`REQ-DSL-006`)
```typescript
interface TriggeredAbilityAST extends AbilityBase {
  kind: "TRIGGERED";
  trigger:
    | { mode: "EVENT"; event: EventMatcher }                 // "When/Whenever …"
    | { mode: "PHASE_STEP"; step: StepId; whose: PlayerRef }  // "At the beginning of …"
    | { mode: "STATE"; state: PredicateAST };                 // CR 603.8 state trigger
  interveningIf?: PredicateAST;                               // CR 603.4
  optional?: boolean;                                         // "you may" → CHOOSE_OPTIONAL_TRIGGER
  limit?: { perTurn?: number; perGame?: number };             // "This ability triggers only once each turn"
  targets?: TargetSlotAST[];
  modes?: ModeSetAST;
  effect: EffectInstructionAST[];
}

interface EventMatcher {
  kind: EventKind;                   // closed enum below
  subject?: ObjectSelectorAST | PlayerSelectorAST;
  filters?: Record<string, PredicateAST>;   // e.g. { from: ZONE(BATTLEFIELD), to: ZONE(GRAVEYARD) }
  bind?: Record<string, string>;     // capture event fields into $vars for the effect
  batch?: "EACH" | "ONE_OR_MORE";    // "Whenever a creature…" vs "Whenever one or more creatures…"
}
```
**`EventKind` (closed set, extended only by DSL minor version):**
* **Zone changes:** `ZONE_CHANGE{from,to}` (with the specializations `ENTERS`, `DIES`, `LEAVES_BATTLEFIELD`, `PUT_INTO_GRAVEYARD`, `LEAVES_GRAVEYARD`, `EXILED`, `RETURNED_TO_HAND`), `DRAWN`, `DISCARDED`, `MILLED`, `CYCLED`, `SACRIFICED`, `DESTROYED`.
* **Stack:** `CAST`, `COPIED`, `ACTIVATED`, `TRIGGERED`, `RESOLVED`, `COUNTERED`, `BECAME_TARGET`.
* **Combat:** `ATTACKS`, `ATTACKS_ALONE`, `BLOCKS`, `BECOMES_BLOCKED`, `ATTACKER_UNBLOCKED`, `DAMAGE_DEALT{combat?: bool}`, `DAMAGE_DEALT_BATCH`.
* **Values:** `LIFE_GAINED`, `LIFE_LOST`, `COUNTERS_PUT{kind}`, `COUNTERS_REMOVED{kind}`.
* **Status:** `TAPPED`, `UNTAPPED`, `TRANSFORMED`, `CONVERTED`, `TURNED_FACE_UP`, `PHASED_OUT`, `PHASED_IN`, `ATTACHED`, `UNATTACHED`, `CONTROL_CHANGED`.
* **Keyword actions and designations:** `TOKEN_CREATED`, `SEARCHED_LIBRARY`, `SHUFFLED`, `SCRY`/`SURVEIL`/`EXPLORE`/`VENTURE`/`BLIGHT`/`WATERBEND`/…, `DESIGNATION_GAINED{kind}` (monarch, initiative, prepared, unlocked half, fully unlocked, day/night, city's blessing, ring tempts…), `ROLLED_DIE`, `FLIPPED_COIN`, `PLAYER_LOST`, `PLANESWALKED`, `SPECIAL_ACTION_TAKEN{kind}`.

**Look-back flag (§8.2):** the engine derives the look-back flag from `kind`/`filters` (603.10a–g). A definition SHALL NOT set it by hand. This prevents authoring errors.

**Example: "Whenever this creature deals combat damage to a player, that player discards a card."**
```json
{"abilityId":"a1","kind":"TRIGGERED","functionsIn":["BATTLEFIELD"],
 "trigger":{"mode":"EVENT","event":{"kind":"DAMAGE_DEALT","subject":{"ref":"SELF"},
   "filters":{"combat":true,"recipient":{"pred":"IS_PLAYER"}},
   "bind":{"recipient":"$P"}}},
 "effect":[{"op":"DISCARD","player":"$P","count":{"LITERAL":1},"chooser":"$P"}]}
```
**Example: "Whenever this creature attacks, draw a card."**
```json
{"abilityId":"a1","kind":"TRIGGERED","functionsIn":["BATTLEFIELD"],
 "trigger":{"mode":"EVENT","event":{"kind":"ATTACKS","subject":{"ref":"SELF"}}},
 "effect":[{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}]}
```

#### 14.4.2 Activated Abilities & Restrictions (`REQ-DSL-007`)
```json
{"abilityId":"a2","kind":"ACTIVATED","functionsIn":["BATTLEFIELD"],
 "cost":[{"cost":"PAY_MANA","mana":"{2}"},{"cost":"TAP_SOURCE"}],
 "timing":"SORCERY",                       // INSTANT (default) | SORCERY | YOUR_TURN | COMBAT_ONLY | …
 "limits":[{"perTurn":1}],
 "activateOnlyIf":{"pred":"CONTROLS","selector":{"pred":"HAS_SUBTYPE","value":{"textToken":"CREATURE_TYPE","value":"Wizard"}}},
 "isLoyalty":false,"isManaAbility":false,
 "targets":[{"slot":0,"selector":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"},"min":1,"max":1}],
 "effect":[{"op":"TAP_OR_UNTAP","mode":"TAP","objects":{"ref":"TARGET","slot":0}}]}
```
* **Cost nodes (`CostAST`):** `PAY_MANA`, `PAY_LIFE`, `TAP_SOURCE`, `UNTAP_SOURCE`, `TAP_OBJECTS{selector,count}`, `SACRIFICE{selector,count}`, `DISCARD{selector,count,random}`, `EXILE_FROM_ZONE{zone,selector,count|minTotalMV}`, `REMOVE_COUNTERS{from,kind,amount}`, `PUT_COUNTERS{on,kind,amount}` (e.g. blight N, which puts −1/−1 counters on a creature you control, CR 701.68), `RETURN_TO_HAND{selector}`, `REVEAL{selector}`, `PAY_ENERGY`, `COLLECT_EVIDENCE{n}`, `FORAGE`, `WATERBEND{mana}` (701.67), `LOYALTY{delta}`, and `COMPOSITE[]`.
* **Mana-payment modifiers** (convoke, delve, improvise, assist) are declared as `PaymentMethodModifier` keyword macros. The 601.2h PaymentResolver consults them. They do not change `cost`.

#### 14.4.3 Static Abilities: Characteristic Effects vs Rule Modifications (`REQ-DSL-008`)
A static ability holds one or more **effects**, and each effect is exactly one of the following:
* **`CharacteristicEffect`:** applied in a layer (1a, 1b, 2–6, 7a–7d). Fields: `layer`, `affected` (selector, or `SAME_AS_LAYER(n)` to declare 613.6 lock-in explicitly), `modification`, `isCDA`.
* **`PlayerEffect`:** applied in pass A (613.10). Examples: hexproof or protection for a player, "can't lose the game", "can't gain life".
* **`RuleModification`:** applied in pass B (613.11). It is one kind from the closed set below. Each kind is consulted by exactly one engine procedure (§14.6):

| `RuleModification.kind` | Consulted by | Example text |
| :--- | :--- | :--- |
| `COST_INCREASE` / `COST_REDUCTION` / `COST_MINIMUM` / `COST_SET` | 601.2f calculator | "Spells your opponents cast cost {1} more"; Trinisphere |
| `ALT_COST_GRANT` / `ADDITIONAL_COST_REQUIRE` | 601.2b | "You may pay life rather than mana"; "As an additional cost to cast creature spells, …" |
| `PAYMENT_METHOD` | 601.2h | "You may spend mana as though it were mana of any color" |
| `CAST_PERMISSION` / `PLAY_PERMISSION` | Priority legal-action generator | "You may cast spells from your graveyard"; "You may play lands from the top of your library" |
| `TIMING_PERMISSION` | Same | "You may cast creature spells as though they had flash" |
| `PROHIBITION` | Same, plus targeting, activation, and special actions | "Your opponents can't cast spells during your turn"; "Activated abilities of artifacts can't be activated" |
| `ADDITIONAL_LAND_PLAY` | Special action 116.2a | "You may play an additional land on each of your turns" |
| `MAX_HAND_SIZE` | Cleanup 514.1 | "You have no maximum hand size" |
| `ATTACK_RESTRICTION` / `ATTACK_REQUIREMENT` / `ATTACK_COST` | Declare-attackers solver | "can't attack"; "attacks each combat if able"; Propaganda |
| `BLOCK_RESTRICTION` / `BLOCK_REQUIREMENT` / `BLOCK_COST` / `BLOCK_CAPACITY` | Declare-blockers solver | "can't block"; "must be blocked if able"; "can block any number of creatures" |
| `EVASION` | Declare-blockers solver | "can't be blocked except by two or more creatures" |
| `HASTE_AS_THOUGH` / `ATTACK_AS_THOUGH` | 302.6 check / attack eligibility | "can attack as though it didn't have defender" |
| `DAMAGE_ASSIGNMENT` | 510.1 assigner | "assigns combat damage equal to its toughness"; "as though it weren't blocked" |
| `TARGETING_RULE` | 601.2c / 115 | "can't be the target of spells your opponents control" (hexproof, shroud and protection are keyword macros built from this) |
| `SKIP` | Turn sequencer | "Skip your draw step" |
| `UNTAP_RULE` | 502.3 | "doesn't untap during your untap step" |
| `MANA_RULE` | 500.5, 106 | "mana doesn't empty from your mana pool" |
| `LIFE_RULE`, `DRAW_RULE`, `ZONE_RULE` | Specific procedures | "Players can't gain life"; "can't draw more than one card each turn"; "cards in graveyards lose all abilities" |
| `WIN_LOSE_RULE` | 104 / SBA | "You can't lose the game"; "Your opponents can't win the game" |
| `SBA_RULE` | §9 registry | "The legend rule doesn't apply to …" |
| `STATION_SUBSTITUTE` | 702.184c | "stations permanents using its toughness" |

**Example: Propaganda.** "Creatures can't attack you unless their controller pays {2} for each creature they control that's attacking you."
```json
{"abilityId":"s1","kind":"STATIC","functionsIn":["BATTLEFIELD"],
 "effects":[{"type":"RULE","kind":"ATTACK_COST",
   "attackers":{"pred":"IS_CREATURE"},
   "attackTarget":{"player":{"ref":"CONTROLLER"}},
   "cost":[{"cost":"PAY_MANA","mana":"{2}"}],"per":"EACH_ATTACKER"}]}
```
**Example: "Creatures your opponents control can't block."**
```json
{"abilityId":"s1","kind":"STATIC","functionsIn":["BATTLEFIELD"],
 "effects":[{"type":"RULE","kind":"BLOCK_RESTRICTION",
   "blockers":{"op":"AND","conditions":[{"pred":"IS_CREATURE"},
     {"pred":"CONTROLLED_BY","player":"OPPONENT_OF_CONTROLLER"}]},
   "rule":"CANT_BLOCK"}]}
```
**Example: "This creature can block any number of creatures."**
```json
{"abilityId":"s1","kind":"STATIC","effects":[{"type":"RULE","kind":"BLOCK_CAPACITY",
  "blockers":{"ref":"SELF"},"max":"UNBOUNDED"}]}
```
**Example: Humility.** "All creatures lose all abilities and have base power and toughness 1/1." Both parts lock to the same object set per 613.6.
```json
{"abilityId":"s1","kind":"STATIC","functionsIn":["BATTLEFIELD"],
 "effects":[
  {"type":"CHAR","layer":"6","affected":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"},
   "modification":{"op":"REMOVE_ALL_ABILITIES"}},
  {"type":"CHAR","layer":"7b","affected":{"sameAsLayer":"6"},
   "modification":{"op":"SET_BASE_PT","power":1,"toughness":1}}]}
```
**Example: Max speed.** A static ability gated on the controller's speed (702.178a).
```json
{"abilityId":"s2","kind":"STATIC","condition":{"pred":"PLAYER_HAS_MAX_SPEED","player":"CONTROLLER"},
 "effects":[{"type":"CHAR","layer":"7c","affected":{"selector":"OTHER_CREATURES_YOU_CONTROL"},
   "modification":{"op":"MODIFY_PT","power":2,"toughness":1}}]}
```

#### 14.4.4 Replacement & Prevention Abilities (`REQ-DSL-009`)
```typescript
interface ReplacementAbilityAST extends AbilityBase {
  kind: "REPLACEMENT";
  isSelfReplacement?: boolean;        // CR 614.15 → tier 616.1a
  tierHint?: "CONTROL_ETB" | "COPY_ETB" | "BACK_FACE_ETB";   // validated by toolchain against 616.1b–d
  match: { event: EventKind; filters: Record<string, PredicateAST>; affected: ObjectSelectorAST | PlayerSelectorAST };
  replaceWith: ReplacementOp[];       // MODIFY_EVENT{set fields} | INSTEAD[instructions] | SKIP | PREVENT{amount?}
  optional?: boolean;                 // "you may … instead"
  overrides614_5?: boolean;           // rare: 903.9b-style repeated application
  shield?: { uses: number | "COUNTER_BACKED"; counterKind?: string };
}
```
**Example: Rest in Peace** (replacement half).
```json
{"abilityId":"r1","kind":"REPLACEMENT","functionsIn":["BATTLEFIELD"],
 "match":{"event":"ZONE_CHANGE","filters":{"to":"GRAVEYARD","from":"ANY"},
          "affected":{"pred":"IS_CARD_OR_TOKEN"}},
 "replaceWith":[{"op":"MODIFY_EVENT","set":{"to":"EXILE"}}]}
```
**Example: "As this enters, choose a creature type"** (an ETB replacement with a stored choice, 614.12a).
```json
{"abilityId":"r1","kind":"REPLACEMENT","functionsIn":["STACK","BATTLEFIELD"],
 "match":{"event":"ENTERS","affected":{"ref":"SELF"}},
 "replaceWith":[{"op":"CHOOSE","choice":"CREATURE_TYPE","chooser":"CONTROLLER","store":"chosenType"}]}
```

#### 14.4.5 Effect Instructions (`REQ-DSL-010`)
Every resolving spell or ability runs an ordered list of instructions. Each one SHALL emit `GameEvent`s through the bus (§8.1) and SHALL declare its choices with a `ChoiceSpec` (§14.4.6).
* **Zones:** `DRAW`, `MILL`, `DISCARD`, `MOVE{objects,toZone,position,faceDown?,tapped?,underControlOf?,transformed?}`, `SEARCH_LIBRARY`, `SHUFFLE`, `REVEAL`, `LOOK_AT`, `PUT_ON_TOP_OR_BOTTOM`, `EXILE_UNTIL{duration}`, `PLAY_PERMISSION_GRANT{objects,duration,who,alsoAs?}`.
* **Damage and life:** `DEAL_DAMAGE{source,recipients,amount,divided?}`, `FIGHT`, `BITE`, `GAIN_LIFE`, `LOSE_LIFE`, `SET_LIFE`, `PREVENT{…}`, `DESTROY`, `SACRIFICE`, `REGENERATE`, `COUNTER_SPELL{unless?: CostAST[]}`.
* **Counters and tokens:** `PUT_COUNTERS`, `REMOVE_COUNTERS`, `MOVE_COUNTERS`, `PROLIFERATE`, `CREATE_TOKEN{tokenDef|inline,count,modifiers}`, `CREATE_PREDEFINED_TOKEN{kind}`, `CREATE_COPY_TOKEN{of,except?}`, `POPULATE`, `AMASS`, `INCUBATE`.
* **Stack:** `COPY_SPELL_OR_ABILITY{newTargets?}`, `CHANGE_TARGETS`, `CAST_FROM{objects,withoutPaying?,altCost?}`.
* **State and designations:** `TAP_OR_UNTAP`, `TRANSFORM`, `CONVERT`, `TURN_FACE_UP/DOWN{profile}`, `PHASE_OUT`, `ATTACH{to}`, `GAIN_CONTROL{duration}`, `EXCHANGE_CONTROL`, `GOAD`, `SET_DESIGNATION{kind,value}` (monarch, initiative, prepared, unlocked half, day/night…), `VENTURE`, `RING_TEMPTS`, `INCREASE_SPEED`, `EMPOWER_JACE{n}`, `MELD`, `MUTATE_MERGE`, `MANIFEST`, `CLOAK`, `EXPLORE`, `CONNIVE`, `DISCOVER`, `BLIGHT{n}`, `WATERBEND{cost}`, and so on. Every CR 701 keyword action has exactly one instruction or macro.
* **Continuous effects:** `CREATE_CONTINUOUS_EFFECT{effects: (CharacteristicEffect|PlayerEffect|RuleModification)[], duration: DurationAST, lockAffectedSet: true}`. `lockAffectedSet` defaults to true, per 611.2c.
* **Delayed and reflexive triggers:** `CREATE_DELAYED_TRIGGER{trigger, effect, captures, oneShot}` (603.7) and `REFLEXIVE_TRIGGER{ifActionTaken, effect}` (603.12).
* **Control flow:** `IF{cond,then,else}`, `FOR_EACH{iterator,body}`, `REPEAT{times}`, `REPEAT_WHILE{cond, maxIter}`, `MAY{player, body}`, `UNLESS{payer, cost, body}` ("[A] unless [B]", which per 733.6 counts as mandatory for loop detection), `CHOOSE_MODE`, `VOTE`, and `REMEMBER{key, value}` to write to Memory or the Linked Store.
* **`DurationAST`:** `UNTIL_END_OF_TURN`, `UNTIL_YOUR_NEXT_TURN`, `UNTIL_END_OF_COMBAT`, `UNTIL{step, whose}`, `FOR_AS_LONG_AS{predicate}` (611.2b), `WHILE_SOURCE_ON_BATTLEFIELD`, `WHILE_CONTROLLED_BY{player}`, `PERMANENT` (for the rest of the game), and `PERPETUAL` (extension only).
* **Loop safety.** `REPEAT_WHILE` SHALL declare `maxIter`. Exceeding it raises an EngineFault, never a silent truncation.

#### 14.4.6 Choice Specifications (`REQ-DSL-011`)
Every instruction that involves a decision SHALL carry a `ChoiceSpec`:
* `chooser`: the player who decides;
* `kind`: `OBJECTS | MODE | NUMBER | COLOR | CREATURE_TYPE | CARD_NAME | PLAYER | YES_NO | ORDER | DIVISION | PILE`;
* `candidates`: a selector or enumeration;
* `min`/`max`/`upTo`;
* `hidden`: who can see the choice;
* `random`: whether the PRNG chooses instead of a player;
* `revealTo`.

The engine generates `PendingChoiceRequest`s (§15.3) **automatically** from these specs. Clients never need card-specific logic.

### 14.5 Text Tokens & Layer 3 (`REQ-DSL-012`) (CR 612)
* `face.text` is a stream of `TextToken`s. Each one is literal text, a `RewritableWord{category, value}`, or a `NameRef{self|other}`. The categories are `COLOR_WORD`, `BASIC_LAND_TYPE`, `CREATURE_TYPE`, `NUMBER` (rare) and `CARD_NAME`.
* Every AST node whose meaning depends on a rewritable word SHALL reference the token **by id** instead of embedding the literal value. Example: `{"pred":"HAS_SUBTYPE","value":{"textToken":"t14"}}`.
* A text-changing effect is a layer 3 `CharacteristicEffect` with `{op:"REPLACE_TEXT_WORD", category, from, to}`, e.g. turning every instance of the word "Swamp" into "Island". In layer 3 the evaluator rewrites the token table for the affected object, and every AST node that references those tokens then reads the new value.
* **Example: Mind Bend.** "Change the text of target spell or permanent by replacing all instances of one color word with another or one basic land type with another." This compiles to `CREATE_CONTINUOUS_EFFECT{layer:3, REPLACE_TEXT_WORD}`, with the categories and values chosen on resolution.
* Self-references ("this creature", "~") are `NameRef{self}` and are never rewritten (CR 201.5 family).

### 14.6 Engine Query Points (`REQ-DSL-013`)
Each engine procedure SHALL consult only the effects listed for it:

| Procedure | Consults |
| :--- | :--- |
| Legal-action generation (priority) | `CAST/PLAY/TIMING_PERMISSION`, `PROHIBITION`, `ADDITIONAL_LAND_PLAY`, activated abilities' `timing`/`limits`/`activateOnlyIf`, the special-action registry |
| Casting 601.2b–f | `ALT_COST_GRANT`, `ADDITIONAL_COST_REQUIRE`, `COST_*` |
| Payment 601.2h | `PAYMENT_METHOD`, `PaymentMethodModifier` macros, mana restrictions |
| Targeting | `TARGETING_RULE` (hexproof, shroud, protection), target predicates |
| Declare attackers | `ATTACK_*`, `HASTE_AS_THOUGH`, `ATTACK_AS_THOUGH`, goad designations |
| Declare blockers | `BLOCK_*`, `EVASION`, keyword evasion macros |
| Combat damage | `DAMAGE_ASSIGNMENT`, trample and banding macros, the bus |
| Characteristics | `CharacteristicEffect` (layers 1–7) |
| Player properties | `PlayerEffect` (613.10) |
| Every event | Replacement abilities (§8.1), then trigger matchers (§8.2) |
| SBAs | §9 registry plus `SBA_RULE` |
| Turn sequencing | `SKIP`, `UNTAP_RULE`, extra-turn, phase and step registrations |

**Conformance requirement:** no engine procedure may branch on a `cardDefId`, a card name, or anything else card-specific.

### 14.7 Keyword Macro Registry (`REQ-DSL-014`)
Every keyword ability (CR 702) and keyword action (CR 701) SHALL be defined **once**, as a parameterized macro that expands into the generic nodes above. Macros are part of the engine's rules vocabulary, not card IP. Examples:

| Keyword | Expansion |
| :--- | :--- |
| `FLYING` (702.9) | `RULE{EVASION: can't be blocked except by creatures with flying or reach}` |
| `MENACE` (702.111) | `RULE{EVASION: can't be blocked except by two or more creatures}` |
| `VIGILANCE` (702.20) | `RULE{ATTACK: attacking doesn't cause it to tap}` |
| `HASTE` (702.10) | `RULE{HASTE_AS_THOUGH: self}` |
| `HEXPROOF` (702.11) / `SHROUD` (702.18) | `RULE{TARGETING_RULE}` with an opponent scope (hexproof) or a universal scope (shroud) |
| `PROTECTION FROM [q]` (702.16) | The DEBT bundle: `TARGETING_RULE`, `BLOCK_RESTRICTION`, `PREVENT` replacement, and an attachment-legality predicate |
| `WARD [cost]` (702.21) | `TRIGGERED{BECAME_TARGET by spell/ability an opponent controls → COUNTER_SPELL unless (that player) pays cost}` |
| `TRAMPLE` (702.19) | `RULE{DAMAGE_ASSIGNMENT: trample}` (the 510.1 assigner enforces lethal floors) |
| `DEATHTOUCH` (702.2) | A damage-dealt flag consumed by SBA 704.5h and by the trample lethal computation |
| `CONVOKE` (702.51) | `PaymentMethodModifier{tap untapped creature → pay {1} or one mana of its color}` |
| `STATION` (702.184) | `ACTIVATED{cost: TAP_OBJECTS(another untapped creature you control); effect: PUT_COUNTERS(charge, = tapped creature's power, or its substitute per 702.184c); timing: SORCERY}` |
| `STATION_SYMBOL{N}` (722.2) | `STATIC{condition: charge counters ≥ N; effects: [granted abilities…, optional CHAR{layer 4: add Creature type; layer 7b: set base P/T}]}` |
| `INCREMENT` (702.191) | `TRIGGERED{CAST by you; interveningIf: self is creature AND manaSpent > power OR > toughness; PUT_COUNTERS +1/+1 on self}` |
| `WARP [cost]` (702.185) | `ALT_COST_GRANT(from hand)` + a delayed trigger (exile at the next end step) + a cast permission from exile after this turn |
| `HARMONIZE [cost]` (702.180) | `ALT_COST_GRANT(from graveyard, tap ≤1 creature)` + a cost reduction equal to the tapped creature's power + a leave-the-stack → exile replacement |
| `PARADIGM` (702.192) | Two spell abilities: (first resolution of this name) a delayed trigger for each of your precombat main phases that creates a castable copy in exile; then exile this spell |
| `MUTATE`, `DISGUISE`, `PROTOTYPE`, `ADVENTURE`-style alternatives | Layout-aware alternative-cast macros (§4.2) |

* A macro is parameterized by the values the parser extracts (e.g. `WARD{cost:[PAY_LIFE 3]}`). **The Scryfall `keywords` array is only a cross-check, because it carries no parameters** (§16).
* Each macro SHALL carry its CR reference, and SHALL be covered by the citation CI (REQ-DOC-001) and by at least one dedicated regression test.

### 14.8 Native Effect Escape Hatch (`REQ-DSL-015`)
Some cards can't be expressed in the schema, or can be expressed only with unacceptable complexity. Examples: *Shahrazad* (subgames), *Karn Liberated*'s restart, *Mindslaver*-style player control, and some Un-cards. These MAY use:
```json
{"op":"NATIVE_EFFECT","id":"native.subgame.start","version":2,"params":{}}
```
Rules for native effects:
* A native effect SHALL be a **pure function** `(S_t, params, PRNG) → (events, S_{t+1}, pendingChoices)`, registered in the engine under a stable `id@version`.
* It SHALL emit every change through the event bus, so replacements and triggers still apply.
* It SHALL declare which layers and RuleSet kinds it may affect, so the dependency solver can treat it conservatively.
* Its `id@version` is serialized into the state and the replay (REQ-API-005).
* Using a native effect does not by itself lower a card's support status, but the coverage report (REQ-ING-013) SHALL count native effects separately. The target is fewer than 0.5% of sanctioned-legal cards.

---

## 15. Client-Server Wire Protocol & View Redaction

### 15.1 Three-Channel Contract (`REQ-NET-001`)
All communication between the engine host and any client follows a **three-channel contract**. It is the same whether the transport is WebSockets, gRPC or local IPC:
```
┌──────────────────────┐                                ┌──────────────────────┐
│   STANDALONE ENGINE  │ ── 1. StateSync / EventDelta ─►│   CLIENT (UI / AI /  │
│     (Host / Lib)     │ ── 2. PendingChoiceRequest ───►│     Test Runner)     │
│                      │ ◄─ 3. SubmitAction ────────────│                      │
└──────────────────────┘                                └──────────────────────┘
```
* Every message carries a `stateVersion` and a `stateHash`. A client that sees a gap in versions SHALL request a full `StateSync`.
* Event deltas are **redacted for each viewer** with the same projection $V$ used for state (§2.4). An event that reveals hidden information to one player (e.g. "P1 looks at the top card") reaches the other players only in redacted form ("P1 looked at a card").

### 15.2 Redacted View Schema (`REQ-NET-002`)
When `getPlayerView(sessionId, viewerId)` is called, the engine emits a `RedactedGameStateView` (JSON or Protobuf). It contains everything a UI needs to render the board, and nothing the viewer isn't entitled to see:
```json
{
  "matchId": "match-90210", "stateVersion": 142, "stateHash": "0x8f3a91c04e2b1109",
  "rulesBaseline": "CR-2026-11-06", "sandboxed": false, "viewerId": "PLAYER_1",
  "turnInfo": { "turnNumber": 4, "activePlayerId": "PLAYER_1", "priorityPlayerId": "PLAYER_1",
                "phase": "COMBAT", "step": "DECLARE_ATTACKERS", "dayNight": "DAY",
                "monarch": "PLAYER_1", "initiative": null },
  "players": [{
    "playerId": "PLAYER_1", "life": 38, "inGame": true,
    "manaPool": [{ "color": "B", "amount": 2, "restrictions": [] }, { "color": "C", "amount": 1, "restrictions": ["ARTIFACT_SPELLS_ONLY"] }],
    "counters": { "POISON": 0, "ENERGY": 3, "RAD": 0, "EXPERIENCE": 0 },
    "designations": { "speed": 2, "citysBlessing": false, "ringTemptCount": 1 },
    "landsPlayedThisTurn": 1, "landPlaysAllowed": 1, "maxHandSize": 7,
    "handCount": 5, "libraryCount": 82, "commanderDamageTaken": { "CARD_77": 6 }
  }],
  "zones": {
    "battlefield": [{
      "objectId": "OBJ_409", "cardId": "CARD_12", "cardDefId": "<oracle_id>",
      "definitionHash": "sha256:…", "supportStatus": "VERIFIED",
      "ownerId": "PLAYER_1", "controllerId": "PLAYER_1",
      "evaluatedCharacteristics": {
        "name": "<card name>", "manaCost": "{U}{B}", "manaValue": 2, "colors": ["U","B"],
        "cardTypes": ["CREATURE"], "subtypes": ["Frog"], "power": 3, "toughness": 4,
        "basePower": 1, "baseToughness": 2,
        "abilities": [
          { "abilityId": "a1", "kind": "TRIGGERED", "textSpan": [0, 14] },
          { "abilityId": "a2", "kind": "ACTIVATED", "textSpan": [15, 27], "isActivatableNow": true },
          { "abilityId": "g1", "kind": "KEYWORD", "keyword": "FLYING", "grantedBy": "EFF_311" }
        ]
      },
      "status": { "tapped": false, "flipped": false, "faceDown": false, "phasedOut": false },
      "summoningSick": false, "designations": {}, "memory": {},
      "counters": { "+1/+1": 1 }, "markedDamage": 0,
      "combatState": { "attacking": null, "blocking": [], "blocked": null },
      "attachedTo": null, "attachments": ["OBJ_512"]
    }],
    "stack": [],
    "hands": {
      "PLAYER_1": [ /* full views */ ],
      "PLAYER_2": [ { "objectId": "OBJ_801", "hidden": true },
                    { "objectId": "OBJ_802", "hidden": false, "cardDefId": "<oracle_id>", "knownBecause": "REVEALED" } ]
    },
    "libraries": { "PLAYER_1": { "count": 82, "knownPositions": { "0": "OBJ_930" } } },
    "graveyards": { "PLAYER_1": [], "PLAYER_2": [] }, "exile": [], "command": []
  }
}
```
Notes on the view schema:
* `evaluatedCharacteristics.abilities` is a list of **ability descriptors** `{abilityId, kind, textSpan, keyword?, isActivatableNow?, grantedBy?: EffectId}`, so a UI can render text spans and highlight activatable abilities.
* `designations` includes `speed: null | 1..4`, `ringTemptCount`, `dayNight: "NEITHER" | "DAY" | "NIGHT"` (at game level), `initiative`, and permanent-level designations (`prepared`, `unlockedHalves`, `goadedBy`, `solved`, `classLevel`, …).
* `memory` exposes the per-object choices visible to the viewer, such as the chosen color and the named card.
* Every visible card carries `supportStatus` and `definitionHash`. That way clients can mark cards that are PARTIAL in sandbox or casual modes.
* Library views expose `count`, plus whichever known positions and known contents the viewer is entitled to (§2.4).
* `sandboxed: boolean` and `rulesBaseline` at the root.

### 15.3 `PendingChoiceRequest` Taxonomy (`REQ-NET-003`)
Every request carries: `requestId`, `chooser`, `reason` (a CR reference and source object), `ChoiceSpec`-derived constraints, a `defaultOnTimeout`, and a list of **pre-validated options** wherever the option space is finite and small (fewer than 10,000 options). For larger spaces it carries a **validator contract** instead: the client submits an answer, and the engine accepts it or rejects it with an explanation.

| `ChoiceType` | Context | Payload | Response |
| :--- | :--- | :--- | :--- |
| `PRIORITY_ACTION` | Heartbeat step 3 | Legal `PlayLand`, `CastSpell` (zone of origin, available faces and alternative casts), `ActivateAbility`, `SpecialAction` (all 116.2 kinds, including unlock, foretell, plot, suspend, companion and turn face up), and `ProposeShortcut`; `canPass` | `PASS_PRIORITY` / `PLAY_LAND{objectId, faceIndex}` / `INITIATE_CAST{objectId, faceIndex, castMode}` / `ACTIVATE{objectId, abilityId}` / `SPECIAL_ACTION{kind, objectId}` |
| `CHOOSE_MODES` | 601.2b, trigger insertion | min, max, repeats allowed, per-mode costs (spree, tiered) | `SELECT_MODES` |
| `CHOOSE_COSTS_AND_X` | 601.2b | Alternative and additional costs, X range, hybrid/Phyrexian options, harmonize creature | `CONFIGURE_COSTS` |
| `CHOOSE_TARGETS` | 601.2c, trigger insertion, 707.10c new targets | Target slots with legal IDs; `mayKeepIllegal` flag for copies | `SELECT_TARGETS` |
| `DIVIDE_VALUE` | 601.2d, 510.1c–d | total; minimum per recipient (1 for spells; 0 for blockers; trample lethal floors computed against the other assignments in the same step) | `SUBMIT_DIVISION` |
| `PAY_MANA_AND_COSTS` | 601.2g–h, attack and block costs, ward, "unless" | Locked total, payment-method candidates (convoke, delve, improvise, waterbend…), mana abilities, auto-pay suggestion | `PAY_COSTS` / `DECLINE_OPTIONAL_PAYMENT` / `CANCEL_AND_ROLLBACK` |
| `DECLARE_ATTACKERS` | 508.1 | Eligible attackers → legal attack targets for each attacker (players, planeswalkers, battles); requirements and the maximum number satisfiable; costs | `SUBMIT_ATTACKERS` |
| `DECLARE_BLOCKERS` | 509.1 | Eligible blockers → attackers each can block; capacity; menace and requirement constraints | `SUBMIT_BLOCKERS` |
| `ORDER_SIMULTANEOUS_TRIGGERS` | 603.3b | This player's waiting triggers | `ORDER_TRIGGERS` |
| `CHOOSE_OPTIONAL_TRIGGER` | "You may" triggers and "may" instructions | Description | `YES` / `NO` |
| `CHOOSE_REPLACEMENT_EFFECT` | 616.1 | Event description + applicable effects in the current tier | `SELECT_REPLACEMENT` |
| `SELECT_OBJECTS_FROM_ZONE` | Tutor, discard, sacrifice, legend rule, mulligan bottom | candidates, min/max | `SUBMIT_OBJECT_SELECTION` |
| `ORDER_CARDS` | Scry, surveil, "in any order", graveyard order (731.3a) | Cards, destinations (top/bottom/graveyard) | `SUBMIT_ORDERING{top[], bottom[], graveyard[]}` |
| `CHOOSE_PLAYER` | "Choose an opponent", battle protector (704.5x–y), 800.4g reassignment | Legal players | `SUBMIT_PLAYER` |
| `CHOOSE_OPTION_OR_VALUE` | Color, creature type, number, sector, vote, anchor word | `optionType`, `allowedValues` | `SUBMIT_CHOICE_VALUE` |
| `CHOOSE_CARD_NAME` | "Choose a card name" | Validator contract (any name in the database, including either face of a DFC (712.16) or a prepare spell name (723.5)) | `SUBMIT_CARD_NAME` |
| `CHOOSE_UNTAP_SET` | 502.3, when an effect allows choosing | Permanents | `SUBMIT_SET` |
| `SHORTCUT_RESPONSE` | 733.2b | The proposed sequence | `ACCEPT` / `SHORTEN{atIndex}` |
| `MULLIGAN_DECISION` | 103.5 | Hand, mulligan count, cards to bottom | `KEEP_HAND{bottom[]}` / `TAKE_MULLIGAN` |
| `PREGAME_ACTION` | 103.6 (e.g. Leylines, companion reveal) | Eligible cards | `SUBMIT_PREGAME` |

### 15.4 Domain Event Stream (`REQ-NET-004`)
Every `submitAction`, `concede`, `submitTimeout` and `executeSandboxCommand` call returns an ordered array of redacted `EngineDomainEvent`s alongside the updated state view. The event types include:
* `TriggerPutOnStackEvent`;
* `TriggerRemovedEvent{reason: INTERVENING_IF_FALSE | NO_LEGAL_TARGETS}`;
* `CostPaidEvent`;
* `DesignationChangedEvent`;
* `TransformEvent`, `MergeEvent`, `MeldEvent`;
* `ControlChangedEvent`;
* `AttachmentChangedEvent`;
* `PlayerLeftGameEvent{reason}`;
* `ShortcutAppliedEvent{iterations}`;
* `EngineFaultEvent{kind, details}`;
* `RulesTraceEvent` (debug builds only), which records the layer order applied, the dependency loops resolved, and the replacement tier choices made.

---

## 16. Card Ingestion, Authoring & Coverage Pipeline

### 16.1 Purpose & Key Facts
This section answers the question: *"Where does the executable behavior of an imported card come from?"*
* **Card data sources carry no machine-readable semantics.** Scryfall (and MTGJSON) provide identity, printed characteristics, layout, legality, rulings and **the full current Oracle rules text**. That includes parenthetical reminder text on many cards. For example, Scryfall's text for a ward card reads "Ward {2} (Whenever this creature becomes the target of …)", and station cards carry station reminder text followed by lines such as `3+ | …` and `12+ | …`. Scryfall also provides a `keywords` array, such as `["Flying","Ward"]`, but the array does **not** carry parameters like the ward cost. None of this data says *how* a card interacts with the engine.
* **Behavior must therefore be compiled from the Oracle text** into the §14 AST. That requires a parser, hand-authored overrides, a review workflow and tests. This is how existing engines work:
  * **MTG Arena:** its Game Rules Engine knows only the basic rules. A Python Game Rules Parser converts English rules text into CLIPS rules. Wizards has said publicly that this handles roughly 80% of *newly printed* cards automatically, with engineers extending the parser for the rest and a regression test for each card.
  * **Forge:** about 34,000 hand-written card scripts in a declarative ability DSL with a keyword registry.
  * **XMage:** hand-written Java classes per card, built from reusable effect classes and generated from stubs.
  * **Magarena:** an experimental grammar parsed only about half of all rules text.
* **Planning assumption:** automatic parsing will cover **50–80%** of cards. The rate is higher for modern templating and lower for older cards. The rest must be hand-authored or flagged as unsupported. Coverage will be measured, never assumed.

### 16.2 Pipeline Overview (`REQ-ING-000`)
```
 Scryfall Oracle Cards bulk ─┐                 ┌─► Rulings bulk ─► golden-test seeds
 (MTGJSON AtomicCards x-check)┤                 │
                              ▼                 │
                    [1] IMPORT & NORMALIZE ─────┘   NormalizedCardRecord (per oracle_id)
                              │
                              ▼
                    [2] LAYOUT & FACE MAPPING        LayoutBehavior + faces[]
                              │
                              ▼
                    [3] TOKENIZE ORACLE TEXT         strip reminder text, TextTokens (§14.5),
                              │                      station symbol lines, ability boundaries
                              ▼
                    [4] KEYWORD MACRO BINDING        702/701 macros w/ parsed parameters
                              │
                              ▼
                    [5] GRAMMAR PARSE → AST          per ability: AST + confidence + residue
                              │
          ┌───────────────────┼────────────────────────┐
          ▼                   ▼                        ▼
 [6a] CLIENT OVERRIDES                            [6c] NATIVE EFFECT binding
 (supplied by the client, keyed by oracle_id         (§14.8)
  + oracleTextHash; validated + smoke-tested;
  REQ-ING-007, REQ-ING-016)
          └───────────────────┼────────────────────────┘
                              ▼
                    [7] STATIC VALIDATION            schema, CR-consistency lints, 605 mana-ability
                              │                      check, 616.1 tier lint, text-span coverage
                              ▼
                    [8] TEST GENERATION & RUN        .pzl smoke tests + rulings-derived tests
                              │
                              ▼
                    [9] STATUS ASSIGNMENT            VERIFIED / AUTO_PARSED / PARTIAL / UNSUPPORTED
                              │
                              ▼
                    [10] PUBLISH CardDatabaseArtifact (content-addressed, signed) + Coverage Report
```
The pipeline is an **offline toolchain** (`fineprint-ingest`, REQ-ARCH-003) that **the client runs** on card data it obtains itself; Fine Print distributes the toolchain, never card data (REQ-NFR-004). The runtime engine only loads its output (REQ-ARCH-001, REQ-API-002 `registerCardDatabase`) and the client's overrides (`registerCardOverrides`).

### 16.3 Requirements

**REQ-ING-001 — Source import.**
* The toolchain SHALL import Scryfall's **Oracle Cards** bulk file (one object per `oracle_id`) and the **Rulings** bulk file, and SHALL key every record by `oracle_id`.
* It SHALL NOT crawl the per-card API endpoints. Scryfall's own guidance is to use the bulk files for mass lookups, to cache data for at least 24 hours, and to send roughly 10 requests per second at most to the API.
* MTGJSON `AtomicCards` MAY be imported for cross-validation, mapped through `identifiers.scryfallOracleId`. Any disagreement in a characteristic SHALL be reported.
* Each import SHALL record the source file's `updated_at` time and hash.

**REQ-ING-002 — Normalization.** Map Scryfall fields into a `NormalizedCardRecord`:
| Scryfall field | Normalized to | Notes |
| :--- | :--- | :--- |
| `oracle_id` | `cardDefId` | Stable across printings. Use the printing `id` only for art and printing data. |
| `name`, `card_faces[].name` | face names | Per face. |
| `mana_cost` (per face) | `ManaCostDSL` | Parse the symbol grammar. An empty string means "no mana cost" only where the layout implies it (e.g. a DFC back face). |
| `cmc` | Validation only | **Floating point. Never use it as a rules value.** Recompute mana value from `ManaCostDSL` per 202.3 and flag mismatches. |
| `type_line` (per face) | supertypes/types/subtypes | Split on "—". Validate against the CR 205 vocabulary. |
| `oracle_text` (per face) | Tokenizer input | The source of behavior. |
| `power`, `toughness`, `loyalty`, `defense` | Strings such as `"*"`, `"1+*"` or `"X"` | Non-numeric values require a CDA or an X rule. For station cards, attach the printed P/T to the highest station-symbol striation (722.2b). Station cards have no P/T off the battlefield (722.2c). |
| `colors`, `color_indicator` | Colors | Cross-check against the computed colors. |
| `color_identity` | `colorIdentity` | Recompute per 903.4 and flag mismatches. |
| `keywords[]` | Cross-check set | Not a source of behavior. It carries no parameters. |
| `layout` | `LayoutBehavior` | See REQ-ING-003. |
| `all_parts[]` | `relatedTokens`, `meldPartner` | Token definitions are imported as token records. |
| `legalities`, `game_changer` | `legalities`, format metadata | Ban lists remain versioned FormatConfig data. |
| `digital`, `games` | `digitalOnly` | Gates `ext.digital`. |
| `oracle_tags` (separate bulk file) | Prioritization hints only | Community tags such as "removal". Never a source of behavior. |

**REQ-ING-003 — Layout mapping.** Scryfall `layout` values SHALL map to `LayoutBehavior`. **Several engine layouts are not distinct Scryfall layouts, so they SHALL be detected from type lines and text:**
| Scryfall `layout` | Additional detection | `LayoutBehavior` |
| :--- | :--- | :--- |
| `normal` | — | `NORMAL` |
| `normal` | Station keyword and `N+ \|` lines | `STATION` |
| `split` | Type line contains `Room` | `ROOM` |
| `split` | Otherwise (incl. fuse, aftermath) | `SPLIT` |
| `flip` | — | `FLIP` |
| `transform` | — | `DFC_NONMODAL` |
| `modal_dfc` | — | `DFC_MODAL` |
| `meld` | Partner via `all_parts` | `MELD` |
| `adventure` | Second face's type line contains `Omen` | `OMEN` |
| `adventure` | Otherwise | `ADVENTURER` |
| `prepare` | — | `PREPARATION` |
| `prototype`, `mutate`, `leveler`, `class`, `case`, `saga`, `battle` | — | Same-named behavior |
| `planar`, `scheme`, `vanguard` | Variant-gated | `PLANAR` / `SCHEME` / `VANGUARD` |
| `token`, `double_faced_token`, `emblem` | — | Token or emblem records |
| `augment`, `host` | Un-set mechanics | `UNSUPPORTED` unless an extension is enabled |
| `art_series`, `reversible_card`, `front_card` | — | Not gameplay objects; skipped |
| any unrecognized value | — | Import error, card marked `UNSUPPORTED` |

**REQ-ING-004 — Tokenization.**
* (a) **Strip reminder text.** Remove parenthesized reminder text, but keep the original for display. The toolchain SHALL use a reminder-text detector that knows the few cards where parentheses are meaningful rules text.
* (b) **Replace self-references.** The card's own name, "this creature", "this spell" and similar phrases become `NameRef{self}`.
* (c) **Mark rewritable words** (§14.5).
* (d) **Split into abilities** on line breaks and bullet structure (modes, anchor words), and recognize station-symbol lines (`N+ |`), level bands, chapter numerals and class-level headers.
* (e) **Normalize typographic variants** such as dashes, quotes and minus signs.

**REQ-ING-005 — Keyword macro binding.**
* Every keyword ability or action found in the text SHALL bind to a §14.7 macro, with **parameters parsed from the text**: costs (`Ward—Pay 3 life`), quantities (`Afflict 2`) and qualities (`protection from red`, `hexproof from monocolored`).
* If the text and `keywords[]` disagree, a lint warning SHALL be raised.
* A keyword with no macro makes the card `UNSUPPORTED`.

**REQ-ING-006 — Grammar parser.**
* A **versioned grammar** SHALL compile templated Oracle text into §14 ASTs. It SHALL be a PEG or GLR grammar over normalized tokens, with semantic actions that build AST nodes.
* For each ability it SHALL produce: the AST, a confidence score, and any **residue** (tokens left unconsumed).
* A card is `AUTO_PARSED` only if **every** ability parses with no residue and passes validation (REQ-ING-009) and generated tests (REQ-ING-010).
* Grammar changes SHALL be regression-tested against the entire card corpus available to CI (REQ-NFR-004). A change that alters any previously accepted AST SHALL be reported (aggregate counts and residue patterns only) and called out in the toolchain's release notes, because clients' `VERIFIED` cards re-run their tests against the new output.
* Grammar coverage SHALL be developed in order of measured value: first the most common templates across the active formats' card pools, then everything else. Templating evolves over time, so the grammar SHALL handle Oracle text as currently errata'd, never old printed wording.

**REQ-ING-007 — Client-supplied overrides.**
* **Fine Print SHALL NOT maintain or distribute card definitions.** Definitions for cards the parser cannot handle, or handles wrongly, are supplied by the client as **overrides**, in the §14 DSL, through `registerCardOverrides` (REQ-API-002) or as an input to `fineprint-ingest` when the client builds its database.
* Each override is keyed by `oracle_id` **and** the `oracleTextHash` it was written against, and carries `origin: "CLIENT_OVERRIDE"` and an optional client-defined `overrideSource`.
* An override takes precedence over parser output.
* An override whose `oracleTextHash` no longer matches the current Oracle text in the client's database SHALL be **demoted to `PARTIAL`** and reported (REQ-ING-012, REQ-ING-016).
* Overrides MAY be partial: they can replace just one ability, identified by `abilityId` or by text span, and leave the parser's output for the others.
* Overrides are untrusted input. The engine SHALL validate every override (REQ-ING-009) and run its generated smoke test (REQ-ING-010a) before accepting it, and SHALL reject an override that fails either, with the reasons.

**REQ-ING-008 — Support status and gating.**
| Status | Meaning | Engine behavior (defaults; overridable per `FormatConfig.cardPolicy`) |
| :--- | :--- | :--- |
| `VERIFIED` | A definition (parsed or client override) that passes validation and its smoke test **and** has one or more **client-supplied behavioral `.pzl` tests** (REQ-ING-010b) that pass in this engine version. The hashes of those tests are recorded in `source.verifiedBy`. | Allowed everywhere |
| `AUTO_PARSED` | Parsed completely with no residue, passes validation and generated smoke tests, no behavioral tests yet | Allowed in casual and sandbox play; **rejected** in tournament mode |
| `CLIENT_OVERRIDE` acceptance | A client override that passes validation and its smoke test is assigned `AUTO_PARSED`-equivalent standing (`supportStatus: "AUTO_PARSED"`, `origin: "CLIENT_OVERRIDE"`) until it earns `VERIFIED` | As `AUTO_PARSED` |
| `PARTIAL` | One or more clauses unimplemented (`knownGaps` lists them), or a stale override | **Warns** in casual play, rejected in tournament mode. Sandbox shows a badge |
| `UNSUPPORTED` | No usable definition | **Rejected** by `validateDeck`. Can't be spawned except in sandbox, with a forced warning |

`VERIFIED` is never assigned by declaration: only passing behavioral tests earn it, and it is re-checked whenever the definition, its tests, or the engine's rules baseline changes.

The engine SHALL NOT silently run a card with a definition that is missing or incomplete.

**REQ-ING-009 — Static validation (lints).** Every definition SHALL pass these checks:
* schema validity;
* the mana-ability classification is consistent with 605.1a/b;
* a replacement's `tierHint` is consistent with 616.1b–d;
* static effects declare their layer or RuleSet kind;
* every text span is covered by some ability (full coverage is required for `AUTO_PARSED` and above);
* triggers don't set the look-back flag by hand;
* `REPEAT_WHILE` declares `maxIter`;
* referenced token definitions exist;
* linked abilities share a `LinkId`;
* computed mana value, colors and color identity match the source data.

**REQ-ING-010 — Tests.**
* (a) **Smoke tests**, generated for every card. Cast or play it in a minimal scenario and check that it resolves without engine faults and that every ability becomes available where its `functionsIn` says.
* (b) Every `VERIFIED` card SHALL have at least one **behavioral `.pzl` test**, supplied by the client with the card's definition or override, that asserts its specific effect. The engine SHALL accept such tests through `registerCardOverrides` (or the database build) and run them before granting `VERIFIED`.
* (c) **Golden ruling tests.** `fineprint-ingest` SHOULD offer the client a triage list of rulings (from the Rulings bulk file) that describe engine-observable behavior, as candidates for behavioral tests. Converting them is the client's choice.
* (d) **Interaction packs.** Fine Print's own interaction coverage lives in the synthetic rules regression suite (REQ-TEST-001). Clients MAY supply multi-card scenarios as behavioral tests.

**REQ-ING-011 — LLM-assisted drafting.** **[WITHDRAWN]** Fine Print does not author card definitions (REQ-ING-007); how a client authors its overrides is out of scope. The ID is retired and SHALL NOT be reused.

**REQ-ING-012 — Change detection (errata and new sets).**
* On each import, compute `oracleTextHash = sha256(normalized oracle_text ⊕ type_line ⊕ mana_cost ⊕ P/T/loyalty/defense)` for each face, and compare it with each definition's `source.oracleTextHash`.
* On a mismatch, demote the definition to `PARTIAL`, attach a diff, and list it in the support report (REQ-ING-016). If the card was parser-generated, re-parse it automatically.
* New cards go through the full pipeline. A new keyword needs a new macro (and a citation in REQ-DOC-001) before any card that uses it can rise above `UNSUPPORTED`.
* CR updates that change a keyword's or rule's meaning trigger re-validation of every card that uses that macro.

**REQ-ING-013 — Coverage reporting.**
* Each database built by `fineprint-ingest` SHALL include a coverage report covering:
  * counts by status;
  * counts by format legality (Standard, Pioneer, Modern, Legacy, Vintage, Commander, Pauper, Brawl…);
  * counts by set;
  * the native-effect count;
  * the top parser residue patterns, ranked by how many cards each blocks;
  * the list of `PARTIAL` cards with their `knownGaps`.
* `registerCardDatabase` SHALL return this summary, and `registerCardOverrides` SHALL return it updated.
* Fine Print's CI SHALL also measure parser coverage against the cached card data allowed by REQ-NFR-004 and publish **only the aggregate numbers and residue patterns** (no card names or text), so grammar work can be prioritized.

**REQ-ING-016 — Support report for clients.**
* `getCardSupportReport` (REQ-API-002) SHALL list every card in the registered database whose status is `UNSUPPORTED` or `PARTIAL`, or `AUTO_PARSED` without behavioral tests, with: `cardDefId`, status, `knownGaps`, the unparsed residue with text spans, any failed lints or smoke-test faults, and whether an override is stale (REQ-ING-012).
* The report SHALL be machine-readable and ordered deterministically (by `cardDefId`), so a client can feed it to its own override-authoring process and submit overrides back.
* `validateDeck` results SHALL reference the same report entries for every card that blocks or warns.

**REQ-ING-014 — Artifact format & licensing.**
* A `CardDatabaseArtifact` is content-addressed (a sha256 Merkle root over its sorted definitions) and MAY be signed by the client that built it. It contains definitions (parsed and client overrides), token records, the `RulesVocabulary` (subtypes, predefined tokens and keyword macro IDs extracted from the CR), any client-supplied behavioral tests, and the coverage report.
* It is built by the client with `fineprint-ingest` and is **never distributed by Fine Print** (REQ-NFR-004). It is always separate from the engine binary (REQ-ARCH-001).
* Clients that redistribute their databases are responsible for Scryfall's terms and the Fan Content Policy (§1.3). Fine Print's documentation SHALL say so.

### 16.4 Worked Example: Importing One Card End-to-End
**Input** (Scryfall Oracle Cards record, abridged): an Omen card whose `layout` is `adventure`. Face 0 is `Creature — Dragon {5}{U}` with the text "Flying\nWard {2} (Whenever this creature becomes the target …)". Face 1 is `Instant — Omen {1}{U}` with the text "Tap up to one target creature. Draw a card. (Then shuffle this card into its owner's library.)".
1. **Layout:** `adventure`, and face 1's type line contains "Omen", so the layout is **`OMEN`** (721).
2. **Tokenize:** strip both reminder-text spans. Face 0 has 2 abilities and face 1 has 1.
3. **Macros:** `FLYING`, then `WARD{cost:[PAY_MANA {2}]}`. The cost comes from the text, not from `keywords[]`.
4. **Parse face 1:** `[TAP_OR_UNTAP{TAP, target slot 0: creature, min 0, max 1}, DRAW{1}]`. The "shuffle into library on resolution" behavior is supplied by the OMEN layout (721.3d), not by the reminder text.
5. **Validate:** all text spans are covered, and the mana value of the main face, 6, matches `cmc`.
6. **Test:** generated smoke tests cover casting it as the Omen (the card ends up in the library and the library is shuffled) and casting the creature (it has flying and ward). Targeting it with an opponent's spell then triggers ward.
7. **Status:** `AUTO_PARSED`. It becomes `VERIFIED` once the client supplies behavioral tests for it that pass (REQ-ING-008).

### 16.5 Bootstrapping Strategy (non-normative)
1. Implement the keyword macro registry first. It is the cheapest work with the widest reach.
2. Build the grammar for the most frequent effect templates (damage, draw, destroy, exile, counters, tokens, "gets +N/+N until end of turn", ETB and dies triggers), ordered by the aggregate residue patterns from CI's coverage measurement (REQ-ING-013).
3. Make the support report (REQ-ING-016) and override path (REQ-ING-007) excellent early: they are how clients fill every gap the parser leaves.
4. Publish aggregate parser-coverage numbers with every release. Clients decide format availability for their own databases through `FormatConfig.cardPolicy` (e.g. tournament mode only when every legal card is `VERIFIED`).

---

## 17. Nonfunctional Requirements: Architecture, Platforms & Toolchain

These requirements record the architecture and platform decisions that the implementation plan is built on. Rationale and the alternatives considered are in the repository's `docs/DECISIONS.md` ("Architecture — …" sections).

**REQ-NFR-001 — Core language and toolchain.**
* `fineprint-core`, the DSL types, the rules engine and `fineprint-ingest` SHALL be written in **Rust**, in one Cargo workspace.
* The Rust toolchain SHALL be pinned to one exact stable version in `rust-toolchain.toml`; CI and every developer build use that version. Changing it is a recorded decision.
* Unsafe code SHALL be confined to the FFI crate (REQ-NFR-002); every other crate SHALL `forbid(unsafe_code)`.

**REQ-NFR-002 — Bindings shipped in the first release.** All three are core deliverables of milestone 1, not optional:
1. **C ABI:** a `fineprint-ffi` crate exporting the §13.2 surface, with `fineprint.h` generated by cbindgen and checked in CI against the exports.
2. **.NET:** `FinePrint.Core`, a NuGet package targeting **.NET 10 (LTS)**, wrapping the C ABI through P/Invoke with an idiomatic `FinePrint.IRulesEngine` API.
3. **WebAssembly:** `fineprint.wasm` built for `wasm32-unknown-unknown` with **wasm-bindgen**, published as an npm package with generated **TypeScript** type definitions, usable in browsers and Node.js.
* Every binding exposes the full §13.2 API, including `registerCardOverrides` and `getCardSupportReport`.

**REQ-NFR-003 — Supported platforms and CI matrix.** CI SHALL build, run the full test suite and replay the golden replay corpus on every platform below, and SHALL fail if any platform or binding produces a different state hash for any replay (REQ-CORE-006):
| Platform | Target | Bindings exercised |
| :--- | :--- | :--- |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | Rust, C ABI, .NET 10, WASM in Node.js |
| Windows x86_64 | `x86_64-pc-windows-msvc` | Rust, C ABI, .NET 10 |
| macOS arm64 | `aarch64-apple-darwin` | Rust, C ABI, .NET 10 |
| Browsers | `wasm32-unknown-unknown` | WASM in headless Chromium, Firefox and WebKit |

**REQ-NFR-004 — Card-data boundary.**
* Fine Print's repositories, packages and releases SHALL contain **no card definitions, card names, Oracle text, rulings or card art**, with one exception: this SRS and other design documentation MAY name cards as examples of interactions.
* The engine's own tests SHALL use **synthetic test cards** (REQ-TEST-001).
* `fineprint-ingest` is run by clients on card data they obtain themselves (§16). Fine Print SHALL NOT host or publish card databases (`*.fpdb`).
* CI MAY download Scryfall bulk files at most once per 24 hours into a CI cache that is never committed or published, solely to measure parser coverage and run parser conformance checks (REQ-ING-006, REQ-ING-013). Developers MAY keep the same files in a git-ignored local cache.
* Overrides are supplied by clients (REQ-ING-007); Fine Print maintains none.

**REQ-NFR-005 — Card toolchain technology.**
* `fineprint-ingest` SHALL share the DSL type definitions crate with the engine, so a definition the toolchain emits is the type the engine loads.
* The Oracle-text grammar SHALL be written as **pest** (PEG) grammar files, versioned with a `grammarVersion` that is recorded in every parsed definition's `source.parserVersion`.

**REQ-NFR-006 — Serialization and hashing.**
* Card definitions and other content-addressed artifacts SHALL use canonical JSON as defined by **RFC 8785 (JSON Canonicalization Scheme)**; `definitionHash` and artifact roots are **SHA-256** over that encoding.
* MessagePack SHALL be the transport encoding (§14.1); it is never used as the input to a content hash.
* The canonical state hash (REQ-LOOP-001) SHALL be a **Merkle** hash: each rules-relevant component (each zone, object, player, the stack, pending triggers, turn position, PRNG state) hashes its canonical encoding, and parents hash their children's digests in a fixed order. It SHALL support locating the first differing component between two states, for divergence debugging. The digest algorithm is SHA-256 unless the Phase 0 performance spike shows it cannot meet REQ-LAYER-005's budget, in which case a different fixed algorithm MAY be chosen by recorded decision, fixed per engine major version.

**REQ-NFR-007 — Comprehensive Rules text.**
* The CR text SHALL NOT be committed to Fine Print's repositories. CI SHALL fetch the official text for the rules baseline from Wizards of the Coast and cache it.
* The repository SHALL commit the citation index required by REQ-DOC-001: every cited rule number with the SHA-256 of its normalized rule text at the last verified baseline. The citation check compares those hashes with the fetched text.

**REQ-NFR-008 — Test coverage.** Line coverage measured with `cargo-llvm-cov` (and the platform's equivalent for the .NET and WASM wrappers) SHALL be at least **90%** for the rules core (state model, zones, objects and layouts, events, layers, turn structure, casting, SBAs, DSL runtime, API and redaction) and at least **75%** for everything else (bindings, `fineprint-ingest`, CLI and tooling). New public functions SHALL each have at least one test.

**REQ-NFR-009 — Performance reference.** The reference hardware for REQ-LAYER-005 is GitHub's standard hosted `ubuntu-latest` runner. Benchmarks SHALL run in CI and be tracked as a trend (median of repeated runs); a regression of more than 10% against the previous wave's baseline SHALL fail the wave gate. Performance targets are never asserted in unit tests.

**REQ-NFR-010 — Milestone 1 scope.**
* Milestone 1 SHALL deliver **two-player (1v1) constructed play**: every generic rule needed for two-player games, all layouts in REQ-OBJ-002 other than the variant-only ones, the full DSL, all three bindings, the card toolchain, overrides and the support report.
* Deferred to later milestones: multiplayer and leaving the game (REQ-FMT-003), Commander and Brawl (REQ-FMT-002), community formats (REQ-FMT-005), the digital extension (REQ-FMT-004), subgames (REQ-ZONE-002), the casual variants and their layouts (Planechase, Archenemy, Vanguard, Conspiracy, Two-Headed Giant), and ante.
* The data model, API and wire protocol SHALL NOT preclude the deferred features: player sets, APNAP ordering, turn order and attack targets SHALL be written for N players from the start.

**REQ-NFR-011 — Repository, review and CI.**
* The engine lives in the public GitHub repository `donfuldauer/fineprint`. `main` SHALL be protected, with the CI jobs of REQ-NFR-003 as required checks.
* Every change reaches `main` through a pull request reviewed by CodeRabbit, under the repository's `docs/PROCESS.md`.

**REQ-NFR-012 — Licensing.**
* Fine Print's code SHALL be licensed under the **Mozilla Public License 2.0** (`MPL-2.0`). Distributed modifications to Fine Print's files must remain available under MPL-2.0 with the original notices kept; applications may embed or link Fine Print, statically or dynamically, without being open-sourced themselves. The full text is the repository's `LICENSE` file, and every package (crates, NuGet, npm) SHALL declare the SPDX identifier.
* Every source file SHALL carry the MPL-2.0 Exhibit A header: "This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of the MPL was not distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/."
* The README, package metadata and any client-facing documentation that names the supported game SHALL carry the Fan Content Policy disclaimer (§1.3, REQ-ARCH-003).

---

## Appendix A — Common Rules Misreadings (Corrected in This SRS)

Earlier drafts of this specification got these wrong. They are kept as a checklist, because they are easy mistakes for implementers and reviewers to repeat.

| # | Misreading | Correct reading per CR | Section |
| :--- | :--- | :--- | :--- |
| 1 | "Layer 8: player-affecting & rule-modifying effects (CR 613.10–11)" | There is no layer 8. CR 613.1 defines layers 1–7. Effects under 613.10 and 613.11 are applied after the layers. | §6.1, §6.4 |
| 2 | "Face-down status overrides & Prototype values" | Layer 1b is face-down only (613.2b). Prototype values are alternative characteristics that are part of copiable values (719.2a). | §4.2, §6.1 |
| 3 | CDAs apply first "in Layers 4, 5, and 7a" | Layers 2–6 (613.3), plus 7a, which consists only of CDAs (613.4a). | §6.2 |
| 4 | Lock-in rules omitted | Added 613.6 lock-in across layers, and 611.2c/611.3a lock-in versus re-evaluation. | §6.3 |
| 5 | Humility + Opalescence given as a dependency example | This is a timestamp case. Blood Moon + Urborg is the dependency example. | §6.2 |
| 6 | Pick the earliest member of a cycle | A dependency loop is applied in timestamp order (613.8b) and re-evaluated after each application (613.8c). | §6.2 |
| 7 | 616.1a–d, with d = "all other" | a self-replacement, b control ETB, c copy ETB, **d back face up ETB**, e any, f repeat, g contained events. | §8.1 |
| 8 | Included Crew/Saddle/Station triggers | 603.10a–g (leaves the battlefield, sacrifice, leaves a graveyard, public object to hand or library, phase out, unattach, control loss, countered, player loses, planeswalk away). Crew, saddle and station are not look-back triggers. | §8.2 |
| 9 | Ward checked as a targeting restriction, and listed among cast triggers | Ward is a triggered ability on becoming a target (702.21a). | §7.1, §14.7 |
| 10 | Convoke, delve, improvise, assist and waterbend applied in 601.2f | These are payment methods used in 601.2h. Waterbend works per 701.67a–b. | §7.1 |
| 11 | A max-floor formula | Reductions in the player's chosen order, a floor of {0}, *then* effects that directly affect the total cost, then lock-in (601.2f). | §7.1 |
| 12 | Day/night, then phasing, then untap | Phasing (502.1), then day/night (502.2), then untap (502.3). | §5.2 |
| 13 | Turn-1 skip "in a two-player game" only | 103.8a–c, configurable through FormatConfig. | §5.2, §11.1 |
| 14 | Cited 500.4 | 500.5. | §5.2 |
| 15 | 704.5u–x lettering; commander SBA at 704.6c | 704.5u space sculptor; v Siege at 0 defense; w non-Siege at 0 defense; x no protector; y illegal protector; z Roles; aa speed. 704.6c is 21 commander damage; 704.6d is the command-zone return. Also added 704.6a/b/e/f. | §9 |
| 16 | Keyword counters included Ward | 122.1b lists 15 kinds (adds decayed, exalted and haste; no ward). | §6.1, §10.2 |
| 17 | Spacecraft/Drone tokens; Lander and Heartwood unsourced | The 111.10a–x list (adds Mutagen and Vibranium; there is no predefined Spacecraft/Drone token). | §10.1 |
| 18 | City's Blessing described as "triggered" | 702.131a (spell ability) and 702.131b (static ability), plus 131d. | §10.3 |
| 19 | Omen treated like Adventure ("on_adventure" exile) | A resolving Omen is shuffled into its owner's library (721.3d). | §4.2, §7.2 |
| 20 | Preparation cited as CR 722 | 723. Added controller-only casting, the no-double-prepare rule, and losing `Prepared` on cast (723.3a–c). | §4.2 |
| 21 | Station "striation threshold" | Station symbols `{N+}` (722.2). The station keyword is 702.184. | §4.2, §14.7 |
| 22 | Meld under CR 712.4 as a DFC | Meld cards are not DFCs (713.1). Meld is CR 713 and 701.42. | §4.2 |
| 23 | "MDFCs cannot transform" (cited 701.28) | Wrong under the current CR. Any permanent represented by a double-faced card or token can transform (701.27a), and MDFCs may have transform or convert abilities (712.3). A permanent can't transform into an instant or sorcery face (701.27e). 701.28 is Convert. | §4.2 |
| 24 | Manifest cited 701.34 | 701.34 is Proliferate. Manifest is **701.40**. | §4.3 |
| 25 | Goad cited 701.38 | 701.38 is Vote. Goad is **701.15**. | §5.2 |
| 26 | Rollback cited CR 732 | CR 734 (Handling Illegal Actions), including the 734.1 limits on reversing mana abilities. | §2.5 |
| 27 | Subgame zones "merge back into library/graveyard/exile" | Every subgame card returns to the main-game **library**, which is then shuffled (730.5). Section is CR 730 (was cited as 728). | §3.2 |
| 28 | Several departures from 800.4 (e.g. a "phantom turn" cited to 800.4i) | 800.4a–p as written. A turn without an active player is 800.4j. | §11.4 |
| 29 | Oathbreaker cited as "CR 906" | Not in the CR. Casual variants run 900–905. | §11.3 |
| 30 | Perpetual "evaluated at the base of Layer 1" presented as a rule | Digital mechanics aren't in the CR. Perpetual is now an explicit design decision in an extension module. | §11.5 |
| 31 | Mandatory loops cited 729.4; K=1000 step heuristic | 104.4b and 733.4–6. Detection by repeated state hash, with K kept only as a fault valve. | §12.1 |
| 32 | Monarch 724, Initiative 725, Day/Night 730, Mutate 727, Adventure 715, Prototype 718 | Renumbered: 726, 727, 732, 731, 716, 719 (the 2026-11-06 CR inserted 713 Meld). | Throughout |
| 33 | Partner combinations listed "Friends Forever" as a standalone ability | It is one variant of "partner—[text]" (702.124i). Added Doctor's companion (702.124m). | §11.2 |
| 34 | Summoning sickness stored as a boolean | Derived from continuous-control tracking (302.6). | §3.1 |

## Appendix B — Assumptions & Open Questions
1. **Legal basis for distribution.** Fine Print distributes no card data (REQ-NFR-004), which keeps its own repositories and releases outside most of the licensing question. Clients that build and redistribute card databases carry that responsibility. Any commercial product that bundles card data still needs legal review (§1.3).
2. **Parser coverage target.** The planning range is 50–80%, based on public statements about MTG Arena's parser (for new cards) and on Magarena's experience with older text. It must be replaced with the aggregate numbers CI measures (REQ-ING-013) after the first grammar milestone.
3. **Digital extension fidelity.** Arena behavior is not formally specified, so `EXT-DIGITAL` behaviors are best-effort, and each one should cite the evidence it is based on.
4. **Tournament loop and shortcut policy** comes from the Magic Tournament Rules. It is out of scope for the CR baseline and is configured separately (§12.1).
5. **Un-set and Acorn mechanics** (augment/host, stickers beyond CR 123, attractions in non-Un contexts) are `UNSUPPORTED` by default. They would need an `ext.unset` module.

## Appendix C — Source Notes (non-normative)
* Comprehensive Rules text verified against a rules mirror reproducing the CR effective November 6, 2026 (the official TXT is published at magic.wizards.com/en/rules). REQ-DOC-001 requires re-verification against the official file.
* Scryfall data behavior verified on 2026-11-14 against: the live API (layouts `adventure` for Omen cards, `split` for Rooms, `prepare` for preparation cards, `normal` for station cards; `oracle_text` including reminder text; `keywords` without parameters), the bulk-data listing (`oracle_cards`, `rulings`, `oracle_tags`, …), and the API documentation (rate limits, caching, terms of use).
* Industry precedent: Wizards of the Coast, "On Whiteboards, Naps, and Living Breakthrough" (MTG Arena rules engine and parser, 2023); the Forge card-scripting wiki; the XMage development guides and `Utils` generators.
