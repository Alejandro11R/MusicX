# Cadence

Cadence is a lightweight, lightning-fast desktop music launcher for focus sessions. Search or paste a YouTube URL, press Enter, and keep working.

No heavy library, no algorithm feeds, no distractions. Just search → listen → focus.

## Features

- **Instant Search & Direct YouTube URLs:** Query YouTube or paste video URLs / IDs (`youtu.be`, `watch?v=`, Shorts, Music) directly.
- **Queue / Up Next System:** Queue songs with 1-click (`+`), reorder or remove tracks, and enjoy atomic auto-advance to the next song upon track completion.
- **Pomodoro Focus Engine:** Integrated focus timer (25 min Focus / 5 min Short Break / 15 min Long Break) with automatic playback pause during rest intervals.
- **Real-Time Audio FX & Speed Presets:** In-flight audio filters powered by `mpv` (Lo-Fi ☕, Bass Boost 🔊, Loudnorm ⚖️, Slowed+Reverb 🌙, Nightcore ⚡) and speed modulation (`0.85x` – `1.25x`).
- **Apple Music-Style Immersive Player:** Fullscreen cinematic player (`I`) with 50/50 symmetrical layout, responsive hero album art, dynamic ambient backdrop from cover art palette, large scale typography, and interactive transport controls.
- **Live Synced Lyrics Engine (LRCLib & Local LRC):** Sub-second timestamp synchronization at 60 fps (`L`) without IPC polling overhead, smart heuristics for YouTube track matching, multi-tier caching (memory + disk + negative cache), manual lyric search fallback, and interactive line-seeking.
- **Cadence CLI Client:** Complete command-line interface (`cadence-cli`) to play, pause, queue, switch presets, and manage focus sessions directly from your terminal or tmux.
- **Cross-Platform (Linux & Windows):** Native IPC abstraction using Unix Domain Sockets on Linux/macOS and Win32 Named Pipes on Windows (`\\.\pipe\cadence-mpv-*`).
- **Event-Driven Concurrency:** High-performance Rust backend powered by `PlayerOrchestrator` and `mpv` IPC event loop with zero-latency state subscriptions.
- **Liquid Glass UI:** Modern translucent aesthetic with ambient blur, responsive progress scrubbers, volume controls, and track history.
- **Keyboard-First & System Tray:** Complete control without leaving your flow (`Space`, `N`, `P`, `Q`, `F`, `M`, `L`, arrows, `/`) plus tray integration.

## Philosophy

- Validate in the terminal first.
- Build incrementally.
- One responsibility per module.
- No speculative abstractions.
- Concrete code first, abstractions only when needed.

## Stack

- **Framework:** Tauri v2 + Rust
- **Frontend:** React 19 + TypeScript + Vite + Tailwind CSS + Lucide Icons
- **Playback Engine:** `mpv` (controlled via Unix IPC socket event loop)
- **Audio Extraction:** `yt-dlp` (stream URL resolution with TTL caching)

## Development

### Prerequisites

- `mpv` and `yt-dlp` installed and available in `$PATH`.
- Node.js (v18+) and Rust toolchain (`cargo`).

### Commands

```bash
# Install dependencies
npm install

# Start development launcher
npm run tauri dev

# Run frontend tests
npm test

# Run backend tests
cd src-tauri && cargo test --lib -- --test-threads=1

# Check lints and types
npm run build
cd src-tauri && cargo clippy --all-targets -- -D warnings
```

### Cadence CLI Usage

While the Cadence desktop app is running, you can control it directly from any terminal:

```bash
# Check status (music + pomodoro)
npm run cli -- status

# Search and play
npm run cli -- play "lofi beats"
npm run cli -- play "https://youtu.be/..."

# Media controls
npm run cli -- toggle
npm run cli -- next
npm run cli -- prev
npm run cli -- volume 80

# Audio FX filters and speed
npm run cli -- preset lofi
npm run cli -- preset bass_boost
npm run cli -- speed 1.15

# Focus / Pomodoro timer
npm run cli -- focus start
npm run cli -- focus pause
npm run cli -- focus skip
```

## Architecture & UI Documentation

- See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for architectural decisions, concurrency model (`PlayerOrchestrator`), IPC socket management, lyrics service, and incident postmortems.
- See [docs/LYRICS-ARCHITECTURE.md](docs/LYRICS-ARCHITECTURE.md) for the complete technical specification of the lyrics synchronization engine, LRCLib client, cache tiers, and 60 fps rendering budget.
- See [docs/UI.md](docs/UI.md) for the visual identity, Liquid Glass specifications, Apple Music Immersive Mode, typography, motion, and interaction guidelines.
- See [docs/AUDIT-2026-10-28.md](docs/AUDIT-2026-10-28.md) for the comprehensive architecture and performance audit.
- See [REFACTOR_PLAN.md](REFACTOR_PLAN.md) for the roadmap history, auditing milestones, and completed refactoring phases.
