## Variant: Canvas · Synodos Field

### Design stance
The canvas in the Synodos app's own visual language: a warm sand research field
with deep-teal ink, gold and rust signals, chunky offset shadows and 12×12 pixel
sprites. Retro-minimal, drawn from the source of truth the owner named.

### Key choices (extracted from synodos01.lovable.app)
- Field: sand `#ece5d8` with an 8px pixel-dither checker (conic-gradient, 5.5% ink)
- Ink: deep teal `#2a3a38` / `#1d2927`; muted `#8f8b80`; keyline `#c5c1b9`
- Signals: gold `#c79a52`, rust `#b45c3c`, moss `#5f7d6a`
- Type: Geist (sans, weight 600 titles) + Space Mono (metadata, badges, relations)
- Radii 6px; **chunky offset shadows** (`4px 4px 0`), 1px keylines, dashed rules
- Artifacts carry **12×12 pixel sprites** (crispEdges rects): page, receipt tape,
  bar chart, reticle, gear, revision arrows — same sprites in the inspector at 34px
- Interaction unchanged: drag, ■ out-port → in-port typed links, lens filters, zoom

### Trade-offs
- Strong at: immediately legible as *your* design language; light field for
  long sessions; sprite system scales to any future artifact type
- Weak at: light chrome gives less "instrument glow"; needs a shipped pixel font
  (Geist/Space Mono are web-loaded here — the product should vendor both)

### Best for
- The default surface if Synodos is the visual anchor
