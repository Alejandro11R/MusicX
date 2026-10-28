# Cadence — Architecture

This document exists to capture *why*, not *what*. The code already shows what
each piece does; it doesn't show why mpv over GStreamer, why `search()` and
`resolve_audio()` are separate functions, or why a bug that looked like a
Rust/Tokio/Tauri problem turned out to be neither. Write this stuff down while
it's still fresh — six months from now nobody (including the person who wrote
it) remembers why an option was discarded.

## Philosophy

Cadence is not trying to be Spotify. It's a music launcher for focus
sessions: search, press Enter, keep working.

- No library, no playlists, no recommendations, no feed.
- Minimal UI on purpose — the point is to get out of the way, not to be a
  destination.
- Concrete code first. Abstractions (traits, fallback sources, caching
  layers) get added when a second real need shows up, not in anticipation of
  one. See "Decisions" below for a case where this was tested directly.

## Flow

```
React (UI)
   │  invoke() commands / listen() events
   ▼
Tauri Commands & Events    — thin delegation + broadcast channels
   │
   ▼
PlayerOrchestrator         — state machine, queue manager, concurrency coordinator
   ├── TrackResolver       — yt-dlp caching (TTL), concurrency semaphore & circuit breaker
   └── MpvEventService     — dedicated task & IPC event loop over Unix socket
         │
         ▼
        mpv (background child process)
```

Search, queueing, and playback interact through `PlayerOrchestrator`:
1. Search queries or direct YouTube URLs resolve lightweight metadata via `yt_dlp::search`.
2. When playing or queueing, `PlayerOrchestrator` manages track ordering, queue indices, and stream resolution via `TrackResolver`.
3. Commands to `mpv` flow through `MpvEventService`, which maintains a persistent Unix socket, observes properties, and broadcasts state changes reactively back to the frontend.

## Decisions

### mpv, controlled over its own IPC socket

Chosen over rolling a custom player: stable, handles network streams well,
low resource usage, and exposes a documented JSON IPC protocol
(`--input-ipc-server`) instead of requiring a library binding. Cadence spawns
one mpv process at startup (`--idle=yes`, `--no-video`) and keeps it alive
for the life of the app, controlling it entirely through that socket —
`services/mpv_event.rs` is the only module that interfaces with the socket.

The IPC connection is **persistent**, not reopened per command. Recreating
the `BufReader` on every call was tried and rejected early: mpv interleaves
unsolicited events with command replies on the same socket, and a
short-lived reader silently drops whatever was buffered but unread between
calls. A dedicated background event loop reads raw JSON messages from mpv,
dispatches responses to waiters, monitors playback properties (`observe_property`),
and emits clean events across broadcast channels.

### yt-dlp for search and audio resolution

Handles YouTube's extraction logic (which changes often) so Cadence doesn't
have to. Used for two things: turning a text query or direct URL into candidate
videos, and turning a video ID into a direct, playable audio-only stream
URL. Never used to download files — everything is streamed by handing mpv a
URL yt-dlp resolved.

### Direct YouTube URLs & Single Video Resolution

Cadence supports both natural language search and direct YouTube video URLs or IDs
(including `youtu.be`, `watch?v=`, `shorts/`, `music.youtube.com`, and raw 11-char IDs).

- `extract_youtube_video_id()` extracts the ID via pattern matching before querying `yt-dlp`.
- Direct URLs bypass `ytsearch10:` query prefixes and query the exact video directly.
- **The Single Video Deserialization Quirk:** When `yt-dlp --flat-playlist -j` queries a single video URL, the returned metadata JSON has `ie_key: null` rather than `"Youtube"`. Previously, this caused serde deserialization failure. Making `ie_key: Option<String>` with default fallback resolved this bug while maintaining playlist filtering.

### `search()` and `resolve_audio()` are separate functions

Both shell out to yt-dlp, but they return different shapes with different
lifetimes:

- `search()` returns metadata (id, title, duration, thumbnail) — cheap,
  meant to be shown in a list, and stable.
- `resolve_audio()` returns a direct stream URL that YouTube signs with a
  short expiry. To balance responsiveness and quota limits, signed URLs are
  cached with a TTL derived from their `expire` timestamp (capped at 30 min,
  with safety margins).

Merging them would couple "list search results" to "resolve every result's
playable URL immediately," which is both slower and mostly wasted work: the
user picks one of several results, not all of them.

### `--flat-playlist` for search (the incident worth remembering)

Playing a track from the UI intermittently timed out. The investigation,
in order:

1. Ruled out React calling `play()` twice, a bad timeout scope, and a stuck
   `wait_with_output()` — confirmed by reproducing the exact failure from a
   plain `cargo test`, no frontend involved.
2. Instrumented `run_yt_dlp` with per-stage timing instead of guessing.
3. The numbers pointed at one specific step: `resolve_audio` was a
   consistent ~3.4s across every run; `search` ranged from 4.9s to 10.4s,
   and one run hit the 15s timeout outright.
4. Realized `yt-dlp -j` on a search returns **full per-video metadata** —
   every format, every subtitle track, storyboard thumbnails — none of
   which `SearchResult` uses. Measured on a 3-result query:

   | | size | time |
   |---|---|---|
   | `-j` | ~244 KB | ~8.8s |
   | `--flat-playlist -j` | ~6 KB | ~3.5s |

   Repeating the failing scenario 3x after switching to `--flat-playlist`:
   search times went from `4.9s / 5.7s / 10.4s` (+ one 15s timeout) to a
   consistent `2.5s / 3.0s / 3.0s`.

The timeout was never raised. The actual problem was asking yt-dlp for far
more than the UI needed, not that 15 seconds was too short.

One trade-off: `--flat-playlist` never returns `thumbnail`. Fixed by
building the thumbnail URL from YouTube's predictable per-video CDN path
(`i.ytimg.com/vi/<id>/hqdefault.jpg`) instead of trusting a field that mode
doesn't provide.

### No SQLite (yet)

There's no data that needs to persist across runs today. SQLite goes in the
moment favorites, history, or playlists become real features — not before,
since designing a schema for hypothetical future data is exactly the kind
of speculative work this project has been deliberately avoiding.

### `PlayerOrchestrator`: Event-Driven Concurrency & Granular State

The legacy `PlayerService` (which wrapped everything in a single global `Mutex`) was
replaced by `PlayerOrchestrator` (`services/orchestrator.rs`).

- **Granular synchronization:** Instead of blocking playback during long yt-dlp operations,
  locks are split between `current_track`, `queue`, `desired_loop`, and atomic playback positions.
- **Instant status reads:** `status()` reads directly from atomic values and watch channels without
  triggering any synchronous IPC round-trips to mpv.
- **Sub-50ms command dispatch:** Fast path commands (`pause`, `resume`, `stop`, `seek`, `set_volume`)
  execute immediately via `MpvEventService` without waiting on track resolution locks.

### Queue Architecture & Atomic Auto-Advance

Cadence implements an in-memory "Up Next" queue without requiring complex database models:

- **Data structure:** Queue items are tracked as `QueueItem { id: String, track: SearchResult }`,
  managed under a `RwLock<Vec<QueueItem>>` and accompanied by `current_queue_index: RwLock<Option<usize>>`.
- **Atomic progression:** When mpv signals `end-file` with reason `eof`, `handle_end_file()` checks
  whether `desired_loop` is disabled. If so, it advances `current_queue_index` and immediately triggers
  playback of the next track inside the backend.
- **Eliminating race conditions:** Previous designs relied on the frontend listening to `TrackEnded` and
  firing a subsequent `play()` call over IPC. This caused race conditions if the UI was lagging or minimized.
  By handling queue advancement inside `PlayerOrchestrator`, transitions are atomic and immune to frontend state drift.
- **Queue manipulations:** Supported operations include `next()`, `previous()`, `add_to_queue()`,
  `add_to_queue_next()`, `remove_from_queue()`, `clear_queue()`, and `play_queue_index()`. All mutations
  emit `OrchestratorEvent::QueueUpdated` to keep UI components in sync.

### Commands stay thin

Every `#[tauri::command]` function is a one- or two-line delegation to a
service — no branching, no business logic.

### Killing child processes is not automatic

Both mpv and yt-dlp are external processes, and both had a real bug from
assuming Rust's `Drop`/`kill_on_drop` would clean them up:

- **mpv**: `kill_on_drop(true)` only fires if `Drop` actually runs. Tauri's
  normal shutdown path uses `std::process::exit`, which skips destructors —
  confirmed by killing the running app and finding mpv still alive
  afterward. Fixed with an explicit `shutdown()` routine and PID file tracking
  wired to `RunEvent::ExitRequested` and Unix signal handlers.
- **yt-dlp**: `run_yt_dlp`'s `Command` had no `kill_on_drop` at all. When
  `tokio::time::timeout` elapses, it drops the inner future — including the
  `Child` it owns — without killing anything. The process kept running
  after Rust gave up waiting on it, then became a zombie the moment it
  exited with nothing left to reap it. Confirmed with `ps aux` showing a
  `<defunct>` yt-dlp entry. Fixed the same way: `kill_on_drop(true)`.

Neither bug was visible from reading the code in isolation — both only
showed up from running the real app and checking real process state.

### Error messages shown to the user are not `Display` output

`CadenceError` implements `Serialize` (required for Tauri to send an `Err`
to the frontend) via a `user_message()` method, deliberately separate from
`Display`. Raw yt-dlp stderr and mpv protocol errors go to `eprintln!` logs
(`Display`, via `thiserror`'s `#[error(...)]`), never to the UI — the first
version of this did leak yt-dlp's raw "Please sign in..." stderr straight
into the interface.

### Cross-Platform Architecture: Linux & Windows

Cadence is designed to run natively on both Linux and Windows:

- **IPC Transport Abstraction:**
  - **Linux / macOS:** Uses Unix Domain Sockets (`/tmp/cadence-mpv.sock` and `/tmp/cadence-control-<user>.sock`).
  - **Windows:** Uses Win32 Named Pipes (`\\.\pipe\cadence-mpv-<user>` and `\\.\pipe\cadence-control-<user>`).
  - mpv receives `--input-ipc-server=\\.\pipe\...` on Windows and `--input-ipc-server=/path/to/socket` on Unix.
  - Tokio I/O is unified across platforms using `tokio::io::split` over generic `AsyncRead + AsyncWrite`.
- **Process Management & Termination:**
  - Linux uses Unix signal handling (`SIGTERM`) and `/proc` inspects for PID tracking.
  - Windows handles `ctrl_c` and uses `taskkill /PID {pid} /F` for stale process cleanup.

### Audio FX & Preset Engine via `mpv`

Cadence inlines real-time audio manipulation without needing heavy client DSP libraries:

- **Filter Presets:**
  - `Lo-Fi (☕)`: `lavfi=[highpass=f=200,lowpass=f=4000]` cuts harsh highs and muddy sub-bass, producing warm vintage sound that reduces cognitive fatigue.
  - `Bass Boost (🔊)`: `lavfi=[equalizer=f=60:width_type=h:width=50:g=10]`.
  - `Loudnorm (⚖️)`: `lavfi=[loudnorm=I=-16:TP=-1.5:LRA=11]` standardizes fluctuating YouTube volume levels.
  - `Slowed + Reverb (🌙)`: `lavfi=[aecho=0.8:0.88:60:0.4]` combined with `0.88x` speed.
  - `Nightcore (⚡)`: `lavfi=[asetrate=44100*1.2,atempo=1.05]`.
- **Dynamic Speed Modulation:**
  - Speed adjustments (`0.85x` – `1.25x`) are handled via mpv's `speed` property, retaining pitch correction without distortion.
- **Persistence Across Tracks:**
  - Selected presets and speed are stored in `PlayerOrchestrator` (`desired_audio_preset`, `desired_speed`) and automatically re-applied upon new track loads or stream recoveries.

### Focus Engine (Pomodoro)

The `FocusEngine` (`services/focus.rs`) provides productivity sessions synchronized with music playback:

- **Deadline-Based Clock:** Calculates remaining duration using `Instant::now()` against target deadlines, avoiding timer drift when laptops suspend or sleep.
- **Automatic Audio Coordination:** When a focus session finishes, audio pauses automatically during the break period if `auto_pause_on_break` is enabled.
- **Phases:** 25-minute Focus intervals, 5-minute Short Breaks, and 15-minute Long Breaks after every 4 completed sessions.

### Local Control Server & Cadence CLI

Cadence runs an asynchronous background `ControlServer` (`services/control_server.rs`) bound to a user-scoped IPC socket:

- **Client Binary (`cadence-cli`):** A lightweight standalone executable that communicates with the active app using line-delimited JSON.
- **Features:** Allows headless developers to inspect playback, search/play tracks, trigger focus sessions, and switch presets from tmux, Neovim, or custom window manager keybindings.

## Organization

```
src-tauri/src/
├── bin/
│   ├── main.rs       — Desktop GUI entrypoint
│   └── cli.rs        — Standalone Cadence CLI client (cadence-cli)
├── commands/         Tauri-facing adapters (playback.rs, search.rs). Thin delegation.
├── services/         All business logic:
│   ├── orchestrator.rs   — PlayerOrchestrator, queue management, auto-advance, presets
│   ├── mpv_event.rs      — Cross-platform MpvEventService, IPC event loop, property observers
│   ├── yt_dlp.rs         — Direct URL extraction, search, TTL audio cache, circuit breaker
│   ├── focus.rs          — FocusEngine (Pomodoro timer, phase transitions, auto-pause)
│   └── control_server.rs — Cross-platform IPC server for CLI client communication
├── models/           Plain structs/enums shared between backend and frontend
│                     (SearchResult, PlayerState, PlaybackStatus, QueueItem, FocusState).
└── errors.rs         CadenceError — every external-process interaction returns Result<T, CadenceError>.

src/
├── lib/
│   ├── tauri.ts      Thin invoke() wrappers + TS mirrors of Rust models/events.
│   ├── playback.ts   Pure helper functions for durations, clamping, and seeking.
│   └── album-color.ts Canvas-based dominant color extraction for ambient glow.
└── App.tsx           Liquid Glass UI (SearchBar, FocusWidget, QueuePanel, NowPlaying, Audio FX).
```

## Adding a new capability

The order that's worked so far, one commit per step:

1. **Model** — add/extend a plain struct in `models/` if new data needs to
   cross the IPC boundary.
2. **Service** — implement the behavior in `services/`, tested against the
   real binary/process it talks to (mpv, yt-dlp) rather than a mock. Every
   external-process test in this codebase does this — it's what caught both
   zombie-process bugs above.
3. **Command** — a thin `#[tauri::command]` wrapper, only after the service
   is proven with `cargo test`.
4. **Frontend** — a wrapper in `src/lib/tauri.ts`, then the UI.

Verify at each layer before moving to the next: `cargo build` / `cargo
test` / `cargo clippy --all-targets`, all clean, before wiring it into
anything above it. This is slower per-commit than building top-down, but it
means a bug is always isolated to the layer that was just touched.
