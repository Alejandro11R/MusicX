# Local transport policy — preparation only

**Not completed hardening.** The policy module is registered only in Linux/macOS
library tests. Control server, CLI and mpv still use their existing endpoints and
lifecycle. S2/S6 endpoint enforcement is pending the next slice; framing and CSP
are separate work. No wire format or CLI option changes are included here.

## Integration contract for the next slice

- Register `services/local_transport.rs` for Linux/macOS production consumers.
  `runtime_dir(true)` creates/validates `/tmp/cadence-<effective uid>` on Linux,
  `/private/tmp/cadence-<effective uid>` on macOS. Environment identity and temp
  overrides are deliberately ignored; there is no old-global fallback.
- Server and CLI append the identical `control.sock` filename. CLI uses
  `runtime_dir(false)` and `validate_socket` before connecting, never creating or
  repairing state. Private directories must be owned by the effective UID and
  mode 0700. Client sockets must be owned sockets with mode 0600.
- Server acquires `EndpointLease` before `bind().await` and retains the lease for
  its entire listener lifetime. Lockfiles are regular, owned, mode 0600; existing
  unsafe files are rejected, never chmodded. The nonblocking OS lock rejects
  concurrent startup. Lockfiles persist: unlinking them would split lock identity.
- `bind` rejects files, symlinks, unsafe sockets and live listeners. Only a
  connection-refused socket is reclaimed, under the lock after inode/device
  identity revalidation. The asynchronous probe has a 200ms bound. New sockets
  are chmodded 0600 inside the private directory before being returned.
- Each mpv launch/restart calls `session_socket` for a fresh private directory.
  There is no PID-file parsing, process lookup, signalling or launch in this
  module. The supervisor must remove its old PID recovery code, retain its actual
  owned `Child`, stop/wait that child before cleanup, and never target an old PID.
  Call the retained lease's `secure_socket` on the owned mpv-created socket
  before client validation; never call listener `bind` for an mpv-owned socket.
- `remove_socket` is for teardown after stopping the owned listener/Child while
  retaining the lease. It validates type/owner/mode and rechecks identity. It does
  not delete regular files, PID files, lockfiles or session directories. Consumer
  teardown may remove its now-empty session directory; crash leftovers are not
  automatically swept by this preparation.

## Security and evidence limits

Trust assumes hostile other users, not hostile processes sharing the effective
UID or root. Standard-library metadata checks are **not openat/race-proof**.
Root-owned sticky shared temporary roots are allowed as ancestors; symlinks,
non-directories, foreign owners and non-sticky writable ancestors are rejected.
Unsupported Unix targets have no registered implementation. Windows remains
unchanged: no ACL/SID security claim. Only Linux tests have been executed; macOS
FFI declarations are target-scoped but have not been compiled or run there.

Fixtures create private temporary directories and remove them in teardown. Tests
cover client/server resolver parity, permissions, synthetic wrong-owner metadata
validation, unsafe components, lock/file/symlink rejection, live collision, stale
socket recovery, unique sessions and non-PID cleanup. Environment independence is
currently structural evidence, not an executed spoofed-environment subprocess.
No consumer wiring, real mpv, process cleanup or Windows behavior is proven here.
