# Cadence

Cadence is a lightweight, lightning-fast desktop music launcher for focus sessions. Search or paste a YouTube URL, press Enter, and keep working.

No heavy library, no algorithm feeds, no distractions. Just search → listen → focus.

## Features

- **Instant Search & Direct YouTube URLs:** Query YouTube or paste video URLs / IDs (`youtu.be`, `watch?v=`, Shorts, Music) directly.
- **Queue / Up Next System:** Queue songs with 1-click (`+`), reorder or remove tracks, and enjoy atomic auto-advance to the next song upon track completion.
- **Event-Driven Concurrency:** High-performance Rust backend powered by `PlayerOrchestrator` and `mpv` IPC socket event loop with zero-latency state subscriptions.
- **Liquid Glass UI:** Modern translucent aesthetic with ambient blur, responsive progress scrubbers, volume controls, and track history.
- **Keyboard-First & System Tray:** Complete control without leaving your flow (`Space`, `N`, `P`, `Q`, `M`, `L`, arrows, `/`) plus tray integration.

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

## Architecture & UI Documentation

- See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for architectural decisions, concurrency model (`PlayerOrchestrator`), IPC socket management, and incident postmortems.
- See [docs/UI.md](docs/UI.md) for the visual identity, Liquid Glass specifications, typography, motion, and interaction guidelines.
- See [REFACTOR_PLAN.md](REFACTOR_PLAN.md) for the roadmap history, auditing milestones, and completed refactoring phases.
