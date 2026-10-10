# Design decision — canvas primitive & Synodos Field visual language

**Date:** 2026-10-09 (owner decisions; supersedes the M1 "home surface" question)
**Status:** DECIDED — M1 UI builds on this; no further palette rounds planned.

## 1. Primitive: open canvas of typed artifacts

Owner decision (2026-10-09): the **open canvas is the home primitive** — not the
carousel strip (001) and not the console-first layout (002). Typed artifacts are
first-class objects on the field:

| Artifact | Meaning | Key typed fields (examples) |
|---|---|---|
| `Draft` | a work-in-progress document (e.g. a Substack piece) | anchor, milestone, state, stale |
| `Receipt` | the record of one operation (route, cost, verdict) | route, cost, verdict, outcome |
| `Evidence` | a measured result or lineage map | dataset, n, contradiction |
| `Scope` | premise + promise for a workstream | promise, audience |
| `Recipe` | a reusable multi-block procedure | blocks, status, budget |
| `Revision` | an accepted edit with before/after hashes | before, after, status |

Relations are **typed edges**, created by dragging the ■ out-port onto another
artifact's ■ in-port: `PRODUCED_BY`, `SUPPORTS`, `CITES`, `GOVERNED_BY`,
`EXECUTED`, `PRODUCED`, `REVISION_OF`, `APPLIED_TO` (+ `LINKS_TO` for ad-hoc).
Lens filters (All / Lineage / Evidence / Workflow) are canvas operations.
Double-click opens the typed inspector.

## 2. Visual language: Synodos Field (sketch 007)

Owner decision (2026-10-09): build M1 on **`design/sketches/007-canvas-synodos-field`**.
Extracted from the owner's Synodos app (`synodos01.lovable.app`): sand field,
deep-teal ink, gold + rust signals, Geist/Space Mono, 6px radii, 1px keylines,
chunky offset shadows, retro node/connector grammar.

Tokens: **`design/tokens/synodos-field.css`** (single source for the M1 UI).

### Pixel-art system (not decoration)
- **12×12 pixel sprites**, integer-grid `<rect>`s with `shape-rendering:crispEdges`,
  one per artifact type (page, receipt tape, bar chart, reticle, gear, revision
  arrows). 16px in card headers, 34px in the inspector. New artifact types add a
  sprite pattern — they do not add a component.
- **8px pixel-dither checker** on the field (conic-gradient at ~5% ink).
- **Hard offset shadows** (`4px 4px 0`) + controls that translate 2px on `:active`.
- Dashed pixel connectors (2/3 dash, butt caps) in ink (light field) with mono
  relation labels.

### Fonts — product requirement
Geist (sans) + Space Mono (mono) are **web-loaded in the sketch only**. The
local-first product must **vendor both** (no runtime CDN). Until vendored, the
UI falls back to system sans/mono.

## 3. Deferred / superseded

- Superseded: 001 Atelier Carousel (home), 002 Workshop Console (home), 003
  Spatial Canvas (old palette), 004 Oxblood Study, 005 Quiet Laboratory,
  006 Moss & Ember (palette round). Their READMEs remain for the record.
- Deferred: **008 Pixel Night** (dark inversion of this language) — a candidate
  for a future "focus/night" theme, not the default.
- Deferred to M1+: the **trace viewer** keeps the console pane idea from 002,
  re-skinned in Synodos Field tokens.

## 4. M1 implications

1. Kernel artifact schema: artifact types + relation kinds as first-class,
   versioned vocabulary (the canvas is the UI of the kernel's typed model).
2. Sprite registry lives in the renderer layer, keyed by artifact type.
3. Fonts vendored into the app bundle (license check: both are OFL — verify and
   record in the license inventory when added).
4. Accessibility: the canvas needs the list-equivalent view + keyboard navigation
   (brief requirement) — designed in M1, not retrofitted.
