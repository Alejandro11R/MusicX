# Local transport policy & CSP hardening

Production consumers on Linux and macOS enforce private local endpoints through
`services/local_transport.rs`, bounded IPC line framing on the control server,
and a non-null WebView Content Security Policy in `tauri.conf.json`.

## Enforced production contract

- `services/local_transport.rs` is compiled in production on Linux/macOS and
  validates `/tmp/cadence-<effective uid>` (Linux) or
  `/private/tmp/cadence-<effective uid>` (macOS). Environment variables (`USER`,
  `UID`, `TMPDIR`) are ignored, with no fallback to global `/tmp/*.sock`.
- `ControlServer` (`services/control_server.rs`) binds `control.sock` through
  `bind_control_at(runtime_root())`, acquiring and holding `EndpointLease` across
  its listener loop and removing only the validated socket on teardown.
- `cadence-cli` (`bin/cli.rs`) connects read-only through
  `connect_control_at(runtime_root())`, rejecting symlinks, non-sockets, foreign
  UIDs or permissions other than `0600` inside an owned `0700` directory.
- `ControlServer::handle_client` bounds incoming request lines to
  `MAX_CONTROL_LINE_BYTES` (16 KiB) and `cadence-cli` bounds response reads to
  64 KiB, preventing unbounded memory allocation over local IPC.
- `MpvPlayer` (`services/mpv_event.rs`) allocates a fresh `0700` session
  directory (`mpv-<pid>-<serial>/ipc.sock`) per launch and reconnect via
  `session_socket`, holds `EndpointLease`, runs `secure_socket` +
  `validate_socket` before connecting, stops and `wait()`s on its owned `Child`,
  and removes only the validated socket under the lease. Legacy `.pid` files,
  `/proc` cmdline scans, and out-of-process signals were removed.
- `src-tauri/tauri.conf.json` enforces a strict WebView CSP restricting scripts
  and defaults to `'self'`, allowing only `'self'`, YouTube thumbnails
  (`https://i.ytimg.com`, `https://*.ytimg.com`), `data:`, `blob:`, and Tauri
  `asset:`/`ipc:` schemes, and disabling `object-src` and `frame-ancestors`.

## Security and evidence limits

Trust assumes hostile other users on the machine, not hostile processes
sharing the effective UID or root. Standard-library metadata checks are **not
openat/race-proof**. Root-owned sticky shared temporary roots are allowed as
ancestors; symlinks, non-directories, foreign owners and non-sticky writable
ancestors are rejected. Unsupported Unix targets have no hardened policy module.
Windows Named Pipes remain unchanged: no ACL/SID security claim.

Only Linux tests and Linux builds have been executed; macOS FFI declarations
are target-scoped but have not been compiled or run there. Automated tests cover
resolver parity, permissions, synthetic wrong-owner metadata validation,
unsafe components, lock/file/symlink rejection, live collision, stale socket
recovery, bounded framing rejection, unique `mpv` sessions, and non-PID cleanup.
