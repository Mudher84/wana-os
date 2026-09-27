# Decision 0004: Private shell-control protocol

- Status: **Accepted** (2026-09-27)
- Scope: Phase 11 shell MVP, launcher keyboard shortcut.

## Context

Global keyboard shortcuts belong to the compositor because it owns physical
input and can recognize them even when an ordinary application has keyboard
focus. The launcher UI belongs to `wana-shell`, which is intentionally a
separate process so a shell crash does not kill applications.

Sending a synthetic key to the shell would couple input focus to global shell
policy. Letting applications bind a control interface would expose privileged
desktop actions.

## Decision

Wana defines the pinned private protocol `wana_shell_control_v1`.

- The compositor advertises it only on the private shell connection created
  by `shell.rs`.
- Public Wayland clients cannot see the global at all.
- Version 1 contains one semantic event: `toggle_launcher`.
- The compositor consumes the launcher shortcut and emits the event.
- The shell owns opening/closing the launcher and its exclusive keyboard
  surface.
- The protocol carries actions, not raw keyboard input.

For the Phase 11 MVP, **Left Super (evdev KEY_LEFTMETA, code 125)** toggles the
launcher. Both press and release are consumed by the compositor so ordinary
clients do not observe half of the shortcut.

## Tests

1. Host: the private shell sees `wana_shell_control_v1`; a public client does not.
2. Unit/source: the protocol is generated from the pinned XML.
3. QEMU: inject Left Super while an ordinary window is focused; `wana-shell`
   logs `launcher opened from shortcut`, maps the overlay and receives
   exclusive keyboard focus.
4. Public `wayland-info` never lists the interface.
