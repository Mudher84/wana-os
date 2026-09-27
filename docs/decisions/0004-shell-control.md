# Decision 0004: Private shell control and launcher shortcut

- Status: **Accepted**
- Date: 2026-09-27
- Scope: Phase 11, shell step 3c completion

## Context

The launcher already opens from the top bar and owns the keyboard while mapped.
A global keyboard shortcut cannot be implemented only inside `wana-shell`,
because ordinary application windows normally own keyboard focus. The compositor
sees every physical key first and already owns the policy for global input.

The shell is a separate privileged Wayland client (decision 0003), so the
compositor needs a minimal private channel for shell-only commands.

## Decision

Add a Wana-owned private protocol:

`wana-shell-control-v1`

Version 1 exposes one shell-only event:

`toggle_launcher`

The protocol global is filtered exactly like layer-shell and the foreign
toplevel list: only the compositor-created shell connection can see or bind it.

The first global shortcut is:

**Super + Space → toggle launcher**

The shortcut is recognized from evdev key state in the compositor, so it is
layout-independent and works identically with Arabic and Latin layouts.

- left or right Super may be used;
- the action fires on Space press only;
- a consumed shortcut is not forwarded to the focused application;
- key release still updates compositor keyboard state;
- if no shell-control resource is bound, the compositor logs the shortcut but
  cannot dispatch it; applications still do not receive the consumed Space.

## Why a private protocol

The command is Wana shell policy, not a generic application protocol.
Making it public would let arbitrary clients impersonate shell actions.
Keeping it private preserves the process isolation of decision 0003 without
moving shell UI into the compositor.

## Protocol lifecycle

The shell binds `wana_shell_control_v1` once at startup.

Requests:

- `destroy` — destructor.

Events:

- `toggle_launcher` — compositor asks the shell to toggle the launcher.

No application IDs, paths or launch commands cross this protocol.

## Tests

1. Protocol tables are generated from the pinned project-owned XML.
2. The shell sees `wana_shell_control_v1`; a public client does not.
3. Unit tests verify Super-left/right + Space detection and non-matching keys.
4. Headless host test injects the shell-control event path.
5. QEMU sends `keydown meta`, `sendkey spc`, `keyup meta`; the launcher
   maps, takes keyboard focus, and a second shortcut closes it.
