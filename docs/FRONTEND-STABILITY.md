# Frontend async ownership

Lyrics requests use latest-intent ownership: changing tracks or issuing a newer
refresh/selection invalidates older success, failure and loading completion.
Manual candidate searches are scoped to their track and view; closing the search
or unmounting the view invalidates pending results. Closing a lyrics view also
prevents its pending selection or refresh from replacing the shared document.

Backend documents retain `track_id` requester identity. The frontend checks that
identity before applying direct results or events. Events have no request generation,
so same-track events are ignored after a direct request starts, including after
its acceptance. Initial event-driven loading remains supported before that point.

Transient failures are logged as failures, not converted to empty successful
lyrics documents. Automatic loading attempts once per track; use refresh to retry.
No public backend event or command shape changes are required.

Deterministic deferred-promise tests execute the production App coordinator,
shared refresh/selection seam, and candidate ownership controller without a DOM.
They cover requester mismatch, immediate TrackStarted ownership despite a pending
older getState readback, manual precedence, lifecycle invalidation, errors,
finalizers and explicit retry. TrackStarted establishes lyrics identity immediately;
state reads capture an observation generation at IPC initiation. TrackStarted
invalidates older reads; its own event-triggered readback cannot restore a
mismatched mpv identity. Later authoritative poll/reconnect reads can establish a
new track even when TrackStarted was missed. Identity is updated at this seam,
not from a delayed React state effect. Manual refresh/selection captures the
view's initiating track ID and rejects a mismatch before acquiring global request
ownership, so stale views cannot suppress newer events or pending requests.
React rendering, visual behavior and real Tauri event timing still require manual
validation.

## Queue undo and Focus settings

Queue-add undo captures the returned queue-item ID, not the last item or media
ID. It checks the current queue before removal: duplicates and intervening adds
survive, and an already removed original is a no-op. Queue events still refresh
the display; success toast and error behavior remain unchanged.

Focus duration and auto-pause setters never fabricate state after a void reply.
The backend's events or a guarded getFocusState readback supply the full state,
including the preserved running/paused interval. New settings intent and Focus
events invalidate older readbacks (including the initial load); obsolete setter
completion/errors cannot publish over them. Current failures remain errors,
not successful settings updates. Controls and backend contracts are unchanged.
Deferred tests execute the actual App callbacks/listener with the production
Focus ownership helper across Focus/short/long breaks, running and paused.
This is not rendered UI or live Tauri timing proof.

## Search invalidation

Every search invocation acquires ownership before checking empty input or local
file extensions. Typing a query, clearing the field or pressing Escape invalidates
pending remote publication and immediately releases its busy indicator. A newer
remote search owns its own busy indicator: older success, failure and finalizers
cannot change it. IPC itself is not cancelled; ownership guards UI publication.
Current AbortError is expected and silent; current real failures remain visible.
Remote results remain visible while a new query is pending; successful remote
search clears selection, while local files select the single result as before.

Deferred tests execute AST-extracted production App search, input-change, clear
and keyboard callbacks with the production ownership helper. They cover local,
empty, clear, Escape, typing and newer remote intent against stale success/error
and completion, plus current success/failure/abort and fresh-result Enter. This
is callback-level proof, not React rendering or live Tauri validation.
