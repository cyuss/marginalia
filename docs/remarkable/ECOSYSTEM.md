# The reMarkable 2 ecosystem: what exists, what it costs us

Status: **desk research, 2026-08-12. Not verified on hardware.**
Bears on: U1, U2, U3, U5, U11, and [ADR-002](../adr/ADR-002-remarkable-ui-and-runtime.md)

> Everything here comes from vendor documentation and community projects, read
> from the outside. The project's rule still applies: a capability leaves
> `UNKNOWN` only after a documented probe on a real device. What this document
> does is replace *guesses* with *sourced claims*, which is a different and
> lesser thing than verification.

---

## 1. Is the reMarkable open source?

**Partly, and not the part that matters most to us.**

reMarkable's own developer documentation describes the OS as a custom Linux
distribution built with the Yocto Project, combining open source components
with proprietary ones.¹ The licence list for the open components is on the
device itself, under *Settings → General → About → Copyrights and licenses*.

**Xochitl — the reading and notebook application — is proprietary, and no
source is available.**¹ That is the piece every third-party integration ends up
negotiating with.

Two further facts from the same page, both load-bearing for this project:

- there is **no official or supported mechanism** for third-party applications
  to run on the device or to reach the display;
- reMarkable states that the user is solely responsible for modifications, and
  reserves the right not to support a modified device.¹

So: the platform is open enough to run our own binary over SSH, and closed
enough that anything touching the screen is unsupported territory.

## 2. The display, and what it costs (U11)

This is the finding that settles [ADR-002](../adr/ADR-002-remarkable-ui-and-runtime.md).

The reMarkable 2 has no framebuffer a third-party process can simply write to.
The community answer is **rm2fb** (`ddvk/remarkable2-framebuffer`), and its
architecture is documented plainly by the project:²

- an **rm2fb server** exposes a drawing API over shared memory and a message
  queue, and it gets there by **injecting itself via `LD_PRELOAD` into a system
  binary** (`remarkable-shutdown`), from where it drives SWTCON;
- an **rm2fb client shim** is `LD_PRELOAD`ed into each application, where it
  intercepts `/dev/fb0` and forwards the calls to the server.

Read against our invariants:

| Invariant | rm2fb |
|---|---|
| 1 — never inject into or depend on private modifications to `xochitl` | The server injects into a system binary and drives the display controller `xochitl` also uses. |
| 2 — never patch or replace system libraries | The mechanism *is* a preloaded replacement library. |
| 4 — install only isolated Marginalia-owned files | The shim must be loaded into other processes to work. |

**Conclusion: there is no known way to draw to a reMarkable 2 screen that does
not violate invariants 1, 2 or 4.** Option A in ADR-002 is not a trade we can
make quietly; it is a decision to abandon three stated promises.

This is not a criticism of rm2fb, which is careful work solving a problem
reMarkable declined to solve. It is simply incompatible with what this project
promised its users.

**Option D — no custom UI, documents as the interface — is therefore not the
cautious choice. It is the only available one.**

## 3. Reading annotations (U2, U3)

Annotation data lives under `/home/root/.local/share/remarkable/xochitl` on the
device.³ Reading it needs developer access (SSH), which the user enables
themselves. That part is unremarkable and matches the plan.

The format is a different story.

### What exists

| Project | Language | Licence | Reach |
|---|---|---|---|
| [`remarkable_lines`](https://docs.rs/remarkable_lines) | **Rust** | **MIT** | v3–v6, "lines, colour and text"⁴ |
| [`rmscene`](https://github.com/ricklupton/rmscene) | Python | MIT | v6, motivated by text extraction⁵ |
| [`remarks`](https://github.com/lucasrla/remarks) | Python | **GPL-3.0** | up to software **2.15** only⁶ |

`remarkable_lines` is the interesting one: Rust, MIT, and therefore
**licence-compatible with this project**. Its author is candid that the format
is proprietary and reverse-engineered, that some fields are guesswork, and that
testing is limited.⁴ Version 0.1.3, documentation coverage in the single
digits.

`remarks` is GPL-3.0. We could not lift code from it into an MIT project
without changing our licence, so it is a reference implementation to learn
from, not a dependency.

### The bad news for U3

`remarks` states plainly that it does **not** work with annotations created by
reMarkable software 3.0 or later, and warns that recent versions changed how
annotation information is stored.⁶ Community reports go further: that there is
no mainstream method for extracting *highlighted text* on current firmware, and
that at least one tool has fallen back to **rasterising each page and looking
for highlight-coloured pixels**.⁷

Take that as a signal, not a verdict — those are secondary sources and may be
stale. But it points the same way in every direction I looked, and it means
**U3 is riskier than the roadmap assumed**. Phase 5 was scoped as "read the
text out, or intersect geometry with the PDF text layer". A third possibility
now has to be considered: that the text is not recoverable from the annotation
files at all on current firmware, and the honest product answer is a highlight
that reports its page and region without claiming to quote.

That would be a real loss. It would not be a reason to guess at the text.

## 4. Package managers and launchers

Toltec is the community package manager; Oxide, draft and remux are launchers
built on top of it. All of them presuppose the display stack in §2, and Toltec
is exactly what invariant 9 says we never install automatically.

Nothing here changes: they are out of scope, and this document records *why*
rather than leaving it as an assertion.

## 5. What this changes

| Question | Before | After |
|---|---|---|
| **U11** display access | open; three candidate routes | **effectively answered.** Every known route violates an invariant. ADR-002 option D is the only one available. |
| **U3** highlight text | "does it carry text, or must we map geometry?" | **worse.** Firmware 3.x changed storage; community tooling has not caught up; text may not be recoverable at all. |
| **U2** annotation read access | open | path confirmed (SSH + the xochitl data directory); still needs a hardware probe. |
| Licensing | unexamined | `remarkable_lines` is MIT and usable; `remarks` is GPL-3.0 and is reference only. |

### Consequences for the plan

1. **ADR-002 can be decided.** Not on preference — on the fact that the
   alternatives are unavailable to a project with these invariants.
2. **Phase 5 needs re-scoping** before it starts, with the possibility that
   quotable highlight text is not achievable on current firmware. The request
   form (ADR-006) does not depend on it; the Annotation Inbox partly does.
3. **`remarkable_lines` is worth a spike** as the parser, behind our own
   versioned adapter, with its guesswork treated as guesswork: unknown
   versions must fail honestly rather than produce a plausible-looking
   misreading.
4. **Nothing here unblocks a device write.** Every capability stays `UNKNOWN`
   until probed.

## 6. What would change these answers

- reMarkable shipping a supported third-party application story;
- a display route that does not require injection;
- a v6 highlight structure that turns out to carry text after all — which a
  single hour with a real device and one highlighted PDF would settle.

That last one is the cheapest and most valuable experiment available to this
project, and it is already written up in
[`DISPLAY_ACCESS_SPIKE.md`](./DISPLAY_ACCESS_SPIKE.md).

---

## Sources

1. [reMarkable Developer — Software stack](https://developer.remarkable.com/documentation/software-stack)
2. [`ddvk/remarkable2-framebuffer` README](https://github.com/ddvk/remarkable2-framebuffer)
3. [`benlongo/remarkable-highlights`](https://github.com/benlongo/remarkable-highlights)
4. [`remarkable_lines` on docs.rs](https://docs.rs/remarkable_lines/latest/remarkable_lines/)
5. [`ricklupton/rmscene`](https://github.com/ricklupton/rmscene)
6. [`lucasrla/remarks`](https://github.com/lucasrla/remarks)
7. [`karismas/ReMarkableHighlightExtractor`](https://github.com/karismas/ReMarkableHighlightExtractor)

Retrieved 2026-08-12. Community projects move; re-check before relying on any
of this.
