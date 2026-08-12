# ADR-006 — Expressing intent on the device without a custom UI

- **Status:** Proposed
- **Date:** 2026-08-12
- **Related:** [ADR-002](./ADR-002-remarkable-ui-and-runtime.md) option D, invariants 1–4, 7, 9; U11

---

## Context

The product decision is that the essential reading workflow runs on the
reMarkable 2, with no desktop and no server. [ADR-002](./ADR-002-remarkable-ui-and-runtime.md)
found that every known route to a *custom* on-device interface collides with a
safety invariant, and that **option D** — no custom UI, the native reader as the
interface — is the only shape that satisfies all fourteen as written.

Option D's cost was named honestly at the time: **it has no obvious way for the
user to say "download this one."** Everything else in option D works — the agent
can sync metadata, generate documents, read annotations, export to Zotero — but
a research companion that cannot accept a per-document instruction is not a
companion.

This ADR proposes the missing mechanism.

## The requirement, precisely

Whatever the mechanism is, invariant 9 and the transfer pipeline demand that it
produce a **deliberate, per-document, auditable user action**. It must be able
to answer, later and in writing: *which human act authorised this download, and
when?* An ambient heuristic ("they opened it twice, they probably want it") is
not an answer.

## Options

### A — Native tags as a command channel

The user applies a tag; the agent polls for it and acts.

- ➕ Native, no generated artefacts.
- ➖ **Requires writing tags** to clear the command after acting, and native tag
  writing is `UNKNOWN` (U5). A command channel that cannot be cleared re-fires
  forever.
- ➖ Tags are a user's own organisational tool. Colonising them for RPC is rude
  and will collide with how people already use them.

### B — A watched folder

The user moves a document into a Marginalia folder.

- ➖ Only expresses "do the thing" for documents already on the device — which
  is the opposite of the problem, since the point is to fetch one that is not.
- ➖ Moving documents means writing to the user's own file organisation.

### C — Companion tap

Fall back to the desktop or a phone for this one action.

- ➖ Fails the product decision outright. This is exactly the dependency the
  standalone decision exists to remove.

### D — The request form ✅

The agent generates a document — an index of the library — with a small empty
box beside each entry. **The user ticks a box with the stylus.** On its next
wake, the agent reads the annotation layer of that document, finds marks that
land inside boxes, and treats each as a request.

```
  Library · generated 12 Aug, 09:14                    generation 01JZ...

  ┌─┐  Attention Is All You Need                        12.4 MB
  └─┘  Vaswani et al. · 2017 · NeurIPS

  ┌─┐  BERT: Pre-training of Deep Bidirectional…         8.1 MB
  └─┘  Devlin et al. · 2018 · NAACL

  ┌─┐  Mamba: Linear-Time Sequence Modeling             ✓ already on device
  └─┘  Gu & Dao · 2023

        Tick a box to download that paper. Marginalia checks
        this page each time it syncs.
```

- ➕ **Requires only annotation *reading*** — the GREEN capability Phase 5 needs
  anyway for highlights. No display access, no `xochitl`, no launcher, no
  package manager. Every invariant holds.
- ➕ The mark **is** the explicit user action: a deliberate physical gesture, on
  a specific row, at a knowable time. It is better evidence than a click,
  because it persists and can be re-read.
- ➕ It uses the device's actual strength — writing on paper — rather than
  fighting for a UI the hardware does not want to give us.
- ➕ Degrades safely: if the agent never runs, the user has a slightly stale
  index and an inked box. Nothing is broken.
- ➖ **Latency.** A request is noticed on the next sync, not instantly. This is
  the real cost, and it must be stated in the document itself so the user is
  not left wondering.
- ➖ Requires generating and re-generating a document, which is a YELLOW write
  (of Marginalia's own document, never the user's).

## Decision

**Adopt D, the request form**, as the interaction mechanism for option D.

The derivation from marks to requests is pure domain logic and is implemented
now, in `marginalia_core::request_form`, testable without any hardware. What
remains hardware-dependent is rendering the form to a PDF and reading the
annotation layer — both already required by other phases.

## The rules the implementation enforces

These exist because a misread mark would mean downloading the wrong paper, or
downloading one twice.

1. **A mark must land in exactly one box.** A stroke touching two boxes is
   `Ambiguous` and is never guessed. The next generation of the form asks again.
2. **Coverage threshold.** A mark must cover a meaningful fraction of the box.
   A pen line crossing the page on its way somewhere else is not a tick.
3. **Generation-scoped.** Every form carries a generation id. A mark on a stale
   copy of the index — the previous version still sitting on the device — is
   ignored. Without this, regenerating the index would re-fire every past
   request.
4. **Idempotent.** A request's key is `generation + entry`. Re-reading the same
   form produces the same key, and the sync journal's uniqueness constraint
   makes the second attempt a no-op. Reading the form twice cannot download
   twice.
5. **One action per entry.** An entry names one document and one action. There
   is no gesture that means "and also do this to everything below".
6. **Nothing is inferred from opening, reading, or annotating a document.** Only
   a mark inside a box is an instruction. Highlighting a paper is not a request
   to do anything.

## Consequences

- Option D becomes a complete product shape rather than one with a hole in it.
  ADR-002 can be decided on its merits rather than on this gap.
- The mechanism is **not** wasted if ADR-002 later chooses a custom UI: the
  request form remains a legitimate offline affordance, and the derivation
  logic is independent of how it is displayed.
- Phase 3's transfer pipeline gains a second source of `ExplicitUserIntent`
  alongside a button. Both produce the same value and pass through the same
  `SafetyManager`; nothing downstream needs to know which it was.
- The form must state its own latency, and must show which requests it has
  already acted on — otherwise a user ticks a box twice because nothing
  appeared to happen.

## What this does not decide

How the form is laid out and rendered (a PDF layer concern, Phase 5), how often
the agent wakes (a lifecycle concern, gated on ADR-002), and whether marks can
express actions beyond download — removal and export are modelled, but only
download is on the Phase 3 path.
