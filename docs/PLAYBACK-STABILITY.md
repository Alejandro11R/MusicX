# Playback stability

Queue entries have unique IDs even when they refer to the same media. Playing
an entry, Next and Previous preserve that entry's position rather than choosing
the first matching media ID. Direct Play still reuses matching queued media or
adds it through the ordinary queue-add path.

Removing the playing entry does not stop its audio. The queue retains a gap at
its former position: Next chooses its former successor, including when the first
entry was removed. Removing the last entry leaves Next at the end (or wraps when
looping). Clearing the queue also leaves current audio playing until Stop or Next.

While a queued selection is resolving, Next advances from that pending entry,
not from the audio still playing. Repeated Next therefore selects successive
queue entries; a late resolution cannot replace the latest selection or Stop.

Removing a queued selection before its URL finishes resolving cancels that
selection without changing the loaded track or its position/pause preference.
The mode immediately returns to Playing or Paused for the retained track, or
Stopped if no track was loaded. Next then navigates from the retained active
entry (and its removal gap, if any). The removed selection's late completion
stays silent and does not re-enqueue it. Once loading has reached mpv, removal
is serialized with those effects and follows the active-entry rule above.

Play and Stop establish a new playback intent before waiting on resolution or
mpv. Old resolutions and recovery attempts cannot commit against a newer intent.
A serialized effects section orders load, restored preferences and Stop; queue
locks are not held while resolving media. Recovery retains its bounded URL retry
budget, desired volume/loop/filter/speed and position/pause restoration.

mpv end-file events with a playlist-entry ID different from the latest start-file
are ignored before clearing transport status. Events without IDs retain legacy
behavior; aborted stop/quit events remain ignored by the orchestrator.

## Verification boundary

Deterministic library tests use an acknowledged fake mpv command channel and a
resolver gate, without network requests or real playback. They cover duplicate
navigation, active removal, direct-play enqueue, queue edits, superseded play and
recovery, Stop while a load acknowledgement is pending, repeated Next during
resolution, and pending-selection removal with retained playing/paused/idle
modes and subsequent Next/Stop. The fake acknowledges commands but does not
prove audible output or emulate mpv's property updates. Existing ignored
integration tests still require real mpv; these tests are not Windows, audible,
or visual validation.
