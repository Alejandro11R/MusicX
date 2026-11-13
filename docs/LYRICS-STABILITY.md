# Lyrics lookup stability

Automatic lookup keeps this priority: local `.lrc` sidecar, embedded ID3 lyrics,
cache, then LRCLIB exact lookup and search fallbacks. Forced refresh skips the
cache, not local lyrics. Prefetch remains silent; explicit requests keep the
existing ready/unavailable event shapes and unavailable message.

## Absence versus provider failure

A 404 or a successful empty search is absence. If all attempted lookups finish
without an error and none finds lyrics, the empty document is negative-cached.
Timeouts, HTTP 429/5xx, malformed JSON and oversized responses are errors, not
negative-cache entries. A later successful fallback may recover an earlier
error. If no fallback succeeds, the first provider error is retained, allowing
the next request to retry instead of reusing a cached absence. Manual search
uses the same distinction and bounded JSON decoding as exact/by-ID lookups.

## Request ownership

Cache keys can share metadata across different track IDs. Every returned
`LyricsDoc` is rebound to its requesting track, including memory hits, disk hits
and coalesced requests. Concurrent forced refreshes share a refresh operation;
normal requests do not cause a forced request to join a cache-reading lookup.
Only the owner of the same in-flight cell can remove that cell, preventing a
late waiter from removing a newer lookup for the same key.

## Deterministic coverage

Run `cargo test --manifest-path src-tauri/Cargo.toml --lib services::lyrics -- --test-threads=1`.
Tests use loopback HTTP responses and isolated system-temp cache directories,
removed during teardown. They cover definitive absence, timeout/429/500/JSON
failure and retry, fallback recovery, requester identity across cache/coalescing,
forced refresh, successful manual/by-ID lookups and local sidecar precedence.
No live provider or Windows/visual validation is implied.
