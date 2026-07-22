# Cadence — Visual Identity

Written before touching Tailwind/shadcn, for the same reason
`ARCHITECTURE.md` was written before the backend had ten more commits on
top of it: decide the shape on purpose, don't let it accrete from whatever
the first component happened to look like.

## Relationship to Bitácora

Reuse the visual *language*, not the interface. Bitácora's design system
(token names, spacing scale, animation timing, shadcn as the component
base) is a good foundation — reusing it means Cadence's typography and
spacing feel like part of the same family of apps instead of a one-off.

The layout does not carry over. Bitácora is a writing tool: panels, a large
editor area, a permanent sidebar, lots of room for content. Cadence is a
launcher: one compact window, no navigation, nothing permanent on screen
except what's actually playing.

## Theme: Glass, not Arc

Bitácora's two themes split roughly into "cool and minimal" (Glass) vs.
"warm and human" (Arc). Arc's coral/beige warmth suits a family-facing
finance app; Cadence is a focus tool, closer to "get in, get music, get
back to work" than to anything domestic. Glass — cool tones, translucent
surfaces, Geist — is the closer fit. Arc's tokens stay available in the
shared system if a second theme is ever wanted, but Glass is the default
and, for now, the only one implemented.

## Layout

A single floating panel, not a full desktop-app window layout:

```
┌──────────────────────────────┐
│ Cadence                   ⚙  │
├──────────────────────────────┤
│ 🔍 Search...                 │
├──────────────────────────────┤
│ ▶ Believer                   │
│ ▶ 505                        │
│ ▶ PIÉNSALO                   │
├──────────────────────────────┤
│ ♪ Playing                    │
│ Junior H — PIÉNSALO          │
│ ⏮  ⏯  ⏭                     │
│ ─────●──────────────         │
└──────────────────────────────┘
```

Three zones, always in this order, nothing else:

1. **Search** — a single input. This is the primary action; it should read
   as the obvious first thing to interact with, not compete with anything
   else for attention.
2. **Results** — a plain list, replaces itself on every search. No
   pagination, no infinite scroll, no persistent history view.
3. **Now playing** — always present at the bottom once something has been
   played, even after the result list is cleared by a new search. Track
   title/artist, transport controls, a slim progress indicator, volume.

No sidebar. No settings screen beyond what genuinely needs one later. No
second window.

## Design tokens

Same names as Bitácora, so nothing here is a new vocabulary to learn:

`background` · `foreground` · `card` · `border` · `primary` · `secondary`
· `destructive` · `muted` · `accent`

One addition specific to Cadence: `playing` — a subtle accent (not the same
as `primary`, which drives the search action) used only on the now-playing
row and the active transport control, so "what's currently making sound"
reads as its own state at a glance rather than blending into ordinary UI
chrome.

No color is ever written directly in a component — only these tokens,
exactly as in Bitácora, so a theme change never touches component code.

## Spacing

Same scale as Bitácora, no new values invented: `4 · 8 · 12 · 16 · 24 · 32`.

Cadence's spacing should run tighter than Bitácora's on average — a
launcher earns its compactness — but every gap still comes from this scale,
never an arbitrary pixel value.

## Typography

Geist, per the Glass theme. One size scale, deliberately small: track
titles, the search input, and body text are close in size — this isn't a
document with a heading hierarchy, it's a short list of things to click.

## Motion

150–250ms, same ceiling as Bitácora. Used for:

- Result list updating after a search.
- The now-playing row appearing/changing track.
- Play/pause icon swap.

Never: animated blur, moving gradients, anything that idles while nothing
is happening. It has to stay light on a modest machine, and more
importantly, a focus tool shouldn't visually compete for attention while
someone is trying to work.

## Components

shadcn/ui as the base, matching Bitácora. What Cadence actually needs is
small: `Input`, `Button`, `Slider` (volume, and later the progress bar),
`ScrollArea` (result list), and glass surfaces on the panel itself and the
now-playing row — per Bitácora's own glass rule, translucency marks
hierarchy, so it does not spread to the whole window background or the
result list rows themselves.

## States worth designing explicitly

- **Empty** (no search yet) — the search input alone, nothing else on
  screen. Not a placeholder dashboard, not a suggestion list.
- **Loading** (`resolve_audio` in flight) — the clicked result shows a
  quiet in-place indicator; nothing jumps to a separate loading screen.
- **Playing / Paused** — the `playing` accent distinguishes these instead
  of relying only on icon shape, since the icon swap alone is easy to miss
  at a glance.
- **Error** — inline, near what failed (the search box or the now-playing
  row), never a modal. Matches `CadenceError::user_message()` on the
  backend: short, plain language, no raw process output.

## What's deliberately not decided yet

No queue UI, no history UI, no keyboard-navigation styling, no
global-shortcut affordance. These were explicitly named as "second phase"
features driven by actual daily use — designing their visuals now would be
the same speculative-work mistake the backend spent several commits
avoiding.
