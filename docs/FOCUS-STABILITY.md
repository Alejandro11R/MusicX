# Focus interval stability

Duration changes apply to the next interval. A running or paused Focus,
short break, or long break retains its current total, remaining time and
deadline. Idle displays the newly configured Focus duration. Existing duration
clamps remain unchanged.

Skip while Idle is a no-op: no completed session, phase event or playback
command. Skipping Focus counts one session; every fourth completed session
starts a long break. Leaving either break starts Focus without changing the
completed count. Reset returns to Idle and preserves that count.

Start is idempotent while running. Pause retains the interval's remaining time;
Start resumes it. Entering a break pauses playback only when auto-pause is
enabled. Entering Focus never automatically resumes playback.

A single transition guard serializes settings, deadline/phase changes, events
and break-entry playback commands. Ticker expiration selects and advances the
phase under that same guard, rather than carrying a stale phase into a later
transition. Production clock samples are taken after acquiring the guard.

## Verification boundary

Rust tests construct the engine without its background ticker and exercise
explicit `std::time::Instant` samples (no sleeps). Playback checks acknowledge
controlled mpv commands, not the fake player's unchanged status watch.

The frontend currently fabricates state after duration updates in `App.tsx`.
That integration is reserved for stabilization T4; this backend unit does not
claim the displayed UI is authoritative yet. No audible, visual or Windows
validation is claimed.
