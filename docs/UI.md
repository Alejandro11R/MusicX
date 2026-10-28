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

A single floating panel with a collapsible sliding Queue drawer:

```
┌──────────────────────────────────────────────┐
│ 🎵 Cadence                     [–] [□] [×]   │
├──────────────────────────────────────────────┤
│ 🔍 Search or paste YouTube URL...            │
│    [YouTube URL badge]                       │
├──────────────────────────────────────────────┤
│ Resultados / Recientes                       │
│ ▶ Believer                               [+] │
│ ▶ 505                                    [+] │
│ ▶ PIÉNSALO                               [+] │
├──────────────────────────────────────────────┤
│ 📋 Cola de reproducción (3)                  │
│   • Track 1 (Reproduciendo)              [×] │
│   • Track 2                          [▶] [×] │
│   • Track 3                          [▶] [×] │
├──────────────────────────────────────────────┤
│ ♪ Now Playing                                │
│ Junior H — PIÉNSALO                          │
│ ⏮  ⏯  ⏭    🔁 [=====●=====] 🔊 📋 (3)      │
└──────────────────────────────────────────────┘
```

Core zones:

1. **Search & URL input** — single input supporting search queries and direct YouTube URLs.
2. **Results & History** — live search results or recent tracks with instant play or `+` (add to queue).
3. **Queue drawer (collapsible)** — displays upcoming tracks, active song indicator, and queue management controls.
4. **Now Playing bar** — persistent playback bar with transport buttons (Previous, Play/Pause, Next, Loop), seek scrubber, volume control, and queue toggle.

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

## Phase 2 Evolutions (Implemented)

The secondary features originally deferred have now been implemented with Liquid Glass styling:

- **Queue Drawer (`QueuePanel`):**
  - Collapsible overlay with backdrop-filter blur (`bg-background/80 backdrop-blur-md`).
  - Badge counter on the queue toggle button in the transport bar.
  - Quick actions per track: play immediately, delete from queue (`×`), or clear all.
  - Active track highlight with subtle `playing` accent.
- **Track Actions in Search & History:**
  - Added an intuitive `+` icon button on each track row to append directly to the queue without interrupting current playback.
- **Keyboard Navigation & Global Shortcuts:**
  - `Space`: Play / Pause toggle.
  - `N`: Next track in queue.
  - `P`: Previous track in queue.
  - `Q`: Toggle Queue panel drawer.
  - `M`: Toggle Mute.
  - `L`: Toggle Loop mode.
  - `Escape`: Close queue panel or dismiss search results.
  - `Left / Right Arrow`: Seek backward / forward (5 seconds).
  - `Up / Down Arrow`: Adjust volume (5% increments).
- **Direct YouTube URL Recognition:**
  - An inline badge signals immediately when a valid YouTube video URL or ID is pasted into the search bar.
