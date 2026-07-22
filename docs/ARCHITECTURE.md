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
   │  invoke()
   ▼
Tauri commands            — thin, no logic, just delegate
   │
   ▼
PlayerService              — owns the mpv connection + "what's playing"
   │                    │
   ▼                    ▼
yt_dlp::search /    MpvPlayer
yt_dlp::resolve_audio     │  JSON over a Unix socket
   │                      ▼
   ▼                     mpv (child process)
YouTube (via yt-dlp)
```

Search and playback are two separate paths that only meet inside
`PlayerService::play()`: resolve a track's audio URL, then hand it to mpv.

## Decisions

### mpv, controlled over its own IPC socket

Chosen over rolling a custom player: stable, handles network streams well,
low resource usage, and exposes a documented JSON IPC protocol
(`--input-ipc-server`) instead of requiring a library binding. Cadence spawns
one mpv process at startup (`--idle=yes`, `--no-video`) and keeps it alive
for the life of the app, controlling it entirely through that socket —
`services/mpv.rs` is the only file that knows the socket exists.

The IPC connection is **persistent**, not reopened per command. Recreating
the `BufReader` on every call was tried and rejected early: mpv interleaves
unsolicited events with command replies on the same socket, and a
short-lived reader silently drops whatever was buffered but unread between
calls. The reader/writer halves live in `MpvPlayer` for the connection's
whole lifetime instead.

### yt-dlp for search and audio resolution

Handles YouTube's extraction logic (which changes often) so Cadence doesn't
have to. Used for two things only: turning a text query into candidate
videos, and turning a video ID into a direct, playable audio-only stream
URL. Never used to download files — everything is streamed by handing mpv a
URL yt-dlp resolved.

### `search()` and `resolve_audio()` are separate functions

Both shell out to yt-dlp, but they return different shapes with different
lifetimes:

- `search()` returns metadata (id, title, duration, thumbnail) — cheap,
  meant to be shown in a list, and stable.
- `resolve_audio()` returns a direct stream URL that YouTube signs with a
  short expiry. It is **never persisted** — a track's URL is re-resolved
  every time it's actually played, including replaying the same track twice
  in a row.

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

### `PlayerService`, not a `Player` trait

`services/player.rs` owns the mpv connection and the currently-loaded track,
and is a concrete struct, not a trait with `MpvPlayer` as its one
implementation. A trait was considered and rejected: there's no second
player backend planned, Rust makes "extract a trait later" a mechanical,
compiler-guided refactor when a second implementation actually shows up, and
an async trait today would mean pulling in `async-trait` and `Box<dyn
Player>` to pay for a flexibility nothing uses yet.

`PlaybackStatus` is derived from `(current_track, mpv's paused flag)` on
every `state()` call rather than tracked as separate mutable state — it
can't drift out of sync with what mpv is actually doing, because there's
nothing to drift.

### Commands stay thin

Every `#[tauri::command]` function is a one- or two-line delegation to a
service — no branching, no business logic. `commands/search.rs` calls
`services::yt_dlp::search` directly rather than through `PlayerService`,
since `PlayerService` has nothing to add there (no cache, no history yet);
a passthrough method would just be indirection with no behavior.

### Killing child processes is not automatic

Both mpv and yt-dlp are external processes, and both had a real bug from
assuming Rust's `Drop`/`kill_on_drop` would clean them up:

- **mpv**: `kill_on_drop(true)` only fires if `Drop` actually runs. Tauri's
  normal shutdown path uses `std::process::exit`, which skips destructors —
  confirmed by killing the running app and finding mpv still alive
  afterward. Fixed with an explicit `MpvPlayer::kill()` wired to
  `RunEvent::ExitRequested`.
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

## Organization

```
src-tauri/src/
├── commands/    Tauri-facing adapters. No logic — one call into a service.
├── services/    All business logic. yt_dlp.rs and mpv.rs never import
│                each other; player.rs is the only place that composes them.
├── models/      Plain structs/enums shared between backend and frontend
│                (SearchResult, PlayerState, PlaybackStatus). pub, so they
│                stay reachable and don't trip Rust's dead_code lint before
│                something outside tests constructs them.
└── errors.rs    CadenceError — every external-process interaction returns
                 Result<T, CadenceError>. #![deny(clippy::unwrap_used,
                 clippy::expect_used)] at the crate root makes this a
                 compiler error to violate, not just a convention.

src/
└── lib/tauri.ts Thin invoke() wrappers + the TS mirror of the Rust models.
                 Field names match the Rust structs exactly (snake_case) —
                 there's no serde rename crossing the IPC boundary.
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
