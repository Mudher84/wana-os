# Decision 0004: Private shell-control protocol and global launcher key

- Status: **Accepted**
- Original decision date: 2026-09-27
- Updated to match the consolidated implementation: 2026-10-02
- Scope: privileged shell control and global launcher policy.

## Context

Ordinary application windows own keyboard focus while they are active, so a
desktop-global launcher shortcut cannot live only inside `wana-shell`. The
compositor receives physical input first and already owns global focus/input
policy.

The shell is a separate privileged Wayland client (Decision 0003). The
compositor therefore needs a deliberately small private channel for shell-only
desktop actions that are not standardized by Wayland.

## Decision

Wana owns the private `wana_shell_control_v1` Wayland protocol.

Version 1 exposes one compositor-to-shell event:

- `toggle_launcher`

The global is visible only on the compositor-created private shell connection.
Public clients cannot discover or bind it.

## Launcher shortcut

The production launcher shortcut is the **Super key**.

- recognition happens in the compositor from evdev key state, so it is
  independent of the active Arabic/Latin keymap;
- the launcher shortcut is consumed by the compositor and is not forwarded to
  the focused application;
- the compositor emits `toggle_launcher` to the private shell-control object;
- the shell opens the launcher when closed and closes it when already open;
- if the shell-control object is temporarily unavailable, the key remains
  reserved rather than leaking through to an application.

The launcher can also be opened by the shell's own bar interaction; that path
does not weaken the private protocol boundary.

## Why private

Layer-shell controls shell surfaces and the foreign-toplevel protocol reports
application windows. Neither protocol defines Wana desktop-global commands.

A public shell-control global would let ordinary clients impersonate desktop
actions. Keeping the protocol private preserves the process isolation between
the compositor, the privileged shell and public applications.

## Protocol lifecycle

The shell binds `wana_shell_control_v1` on its private connection.

Requests:

- `destroy` — release the control object.

Events:

- `toggle_launcher` — ask the shell to toggle the launcher.

No application path, command line, credential, or arbitrary payload crosses the
protocol.

## Validation

The consolidated gates require that:

1. the protocol tables are generated from the project-owned XML;
2. the shell sees `wana_shell_control_v1` while public clients do not;
3. attempts by a public client to guess/bind the hidden global fail;
4. QEMU input triggers `shortcut Super -> shell launcher`;
5. the shell maps the launcher, owns keyboard focus, and launches applications
   through the public client connection without inherited privileged
   descriptors.
