# Phase 37 — Local authentication and screen lock

Status: **IN PROGRESS**

## Implemented

- The production desktop no longer exposes the launcher before local
  authentication succeeds.
- A root-owned `wana-authd` broker exposes `/run/wana/auth.sock` and
  authorizes only uid 0 and the dedicated desktop uid 1000 through Linux
  `SO_PEERCRED`.
- First boot enters credential setup mode; later boots enter login mode.
- Password input is decoded through the compositor-provided xkbcommon keymap,
  supports modifiers/layout text, is bounded to 128 UTF-8 bytes, and is never
  rendered or logged.
- Credentials are stored only as a salted
  `PBKDF2-HMAC-SHA256` verifier with 200,000 iterations under
  `/var/lib/wana/auth/default.cred`.
- Salt comes from `/dev/urandom`; verification uses constant-time comparison.
- The credential directory is root-owned mode 0700 and the credential file is
  created atomically mode 0600 with file and directory fsync.
- Failed verification is delayed by 500 ms.
- The authentication layer is a full-screen privileged shell overlay with
  exclusive keyboard focus, so windows underneath cannot receive keyboard or
  pointer interaction while locked.
- The normal desktop service sets `WANA_REQUIRE_AUTH=1` and depends on the
  ready auth broker.
- `wana-lock` sends a request through a per-user Unix datagram endpoint under
  `/run/user/1000`; the launcher exposes “قفل الشاشة”. Locking preserves
  running applications but restores the full-screen login overlay.
- Bring-up subsystem gates may use the explicit
  `wana.auth-test-bypass=1` kernel flag. Normal production boot does not use
  this flag.
- The final gate uses the same writable disk for two boots: first credential
  setup, then verification on the next boot, followed by an in-session relock.

## Exit gate

`make auth-login-boot-test` must:

1. boot a pristine production disk and reach setup mode;
2. enter and confirm a password as uid 1000;
3. persist only the root-owned verifier on Data;
4. unlock the desktop;
5. boot the same disk again and reach login mode;
6. verify the same password and unlock;
7. launch “قفل الشاشة” and return to the login overlay without terminating the
   existing desktop session.

`make production-session-boot-test` independently proves that a pristine
production image is locked by default and does not expose the launcher.

## Evidence

Implementation and both final gates exist. Evidence is intentionally deferred
to the consolidated final validation pass.
