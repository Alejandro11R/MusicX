# Frontend lyrics ownership

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
