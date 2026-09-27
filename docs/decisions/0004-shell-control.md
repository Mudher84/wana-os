# Decision 0004: Private shell-control protocol and launcher shortcut

- Status: **Accepted** (2026-09-27)
- Scope: Phase 11 shell step 3d.

## Decision

Wana owns a deliberately tiny private Wayland protocol,
`wana_shell_control_v1`, for compositor-to-shell desktop actions that have no
standard protocol.

Version 1 has one event:

- `toggle_launcher`

The compositor exposes this global only to the shell connection created through
the existing private socketpair. Public clients cannot see or bind it.

## Launcher shortcut

The first global shortcut is **Super+Space**.

- The compositor recognizes the evdev sequence globally.
- The Space press/release used by the shortcut is consumed and is not forwarded
  to the focused application.
- The compositor sends `toggle_launcher` to the shell.
- The shell opens the launcher when closed and closes it when already open.
- If the shell is temporarily down, the shortcut remains reserved; after the
  bounded shell restart the newly bound control object receives future actions.

## Why private

Layer-shell controls surfaces and ext-foreign-toplevel-list reports windows;
neither specifies desktop global shortcuts. Sending a synthetic key to the
shell would mix policy with focus and break the isolation model. A minimal,
filtered Wana protocol keeps the policy explicit and testable.

## Security boundary

`wana_shell_control_v1` is marked privileged by the same global filter as
layer-shell and the foreign-toplevel list. A public client does not see it.
