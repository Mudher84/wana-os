# Phase 35 — Power and session lifecycle

Status: **IN PROGRESS**

## Implemented

- Root-owned `wana-powerd` broker exposes `/run/wana/power.sock`.
- The socket is mode 0660, owned by root and the desktop group.
- Peer authorization uses Linux `SO_PEERCRED`; only uid 0 and the Wana
  desktop uid 1000 are accepted.
- Supported operations are deliberately narrow: status, poweroff and reboot.
- The broker syncs filesystems before invoking the Linux reboot syscall.
- The native Arabic `wana-power-ui` requires a second Enter confirmation for
  destructive actions and supports cancellation with Escape.
- The Power application is available from the normal Wana launcher.
- The final functional gate launches Power from the real uid-1000 production
  desktop and requires the broker to authorize uid 1000 before shutdown.

## Exit gate

`make power-ui-boot-test` must boot the production desktop, launch the Power
application through the normal launcher, require the confirmation step, observe
an authorized uid-1000 power request, and power the guest down cleanly.

## Evidence

Implementation and the final gate exist. Evidence is intentionally deferred to
the consolidated final validation pass.
