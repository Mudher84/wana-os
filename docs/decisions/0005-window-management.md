# Decision 0005: Phase 12 window management

- Status: **Proposed — implement only after Phase 11 closes**
- Date: 2026-09-27
- Scope: Phase 12

## Existing state

The Phase 10 compositor already provides:

- mapped xdg_toplevel windows;
- deterministic placement inside the shell's usable area;
- pointer focus and implicit pointer grabs;
- click-to-focus and click-to-raise;
- keyboard focus recovery when a window disappears;
- shell layers above/below windows;
- xdg configure/ack bookkeeping.

The current xdg_toplevel request handler intentionally ignores:

- interactive move;
- interactive resize;
- minimum and maximum size;
- maximize / unmaximize;
- fullscreen / unfullscreen;
- minimize.

Phase 12 adds those semantics without moving policy into wana-shell.

## Window state

Every mapped toplevel gets compositor-owned state:

- `Normal`
- `Maximized`
- `Fullscreen`
- `Minimized`

and:

- current compositor geometry;
- saved normal geometry for restoring from maximized/fullscreen;
- client min/max size constraints;
- pending configure target;
- activated/resizing state bits.

A state-changing request never silently changes the current committed buffer.
The compositor sends an xdg_toplevel configure, records the target against the
xdg_surface configure serial, and applies the new geometry only after the client
acks and commits the corresponding buffer.

## Activated state

Keyboard focus is reflected through xdg_toplevel.configure:

- focused mapped toplevel: `activated`;
- old focus: `activated` removed.

This keeps client decoration/state in sync with the compositor's existing focus
policy.

## Size constraints

`set_min_size` and `set_max_size` are retained per toplevel.

Rules:

- negative sizes are protocol errors;
- zero max dimension means unbounded;
- configure sizes are clamped to the effective constraints;
- contradictory non-zero min/max values are refused rather than guessed.

The compositor also clamps interactive resize to at least 1x1.

## Maximize

`set_maximized` configures the window to the current shell usable area.

The normal geometry is saved exactly once when entering a non-normal state.
`unset_maximized` restores that saved geometry, clamped to the current usable
area and size constraints.

If the shell's exclusive zones change while maximized, a new configure follows
the new usable area.

## Fullscreen

`set_fullscreen` configures the window to the whole output, not the shell's
usable area.

For Phase 12, fullscreen windows remain in the compositor's window stack and
therefore below layer-shell `top` and `overlay` surfaces. This preserves the
layer-shell contract from decision 0003. Shell policy to hide/reveal panels for
fullscreen is a later shell polish decision; it must not be hidden inside the
window manager.

`unset_fullscreen` restores the saved normal geometry or the preceding
maximized state if fullscreen was entered from maximized.

## Interactive move

An xdg_toplevel.move request is accepted only when:

- the supplied wl_seat is seat0;
- the requesting client owns the toplevel;
- a pointer button is currently held on that same window;
- the serial equals the compositor serial from the button press that began the
  implicit grab.

While the grab is active, pointer motion moves the compositor geometry. No
client buffer resize is required.

Releasing the last pointer button ends the operation.

## Interactive resize

xdg_toplevel.resize has the same serial/client/grab validation.

The protocol edge is validated before starting. During the grab:

- pointer motion derives a target rectangle from the initial rectangle;
- min/max size constraints are applied;
- the opposite edge remains fixed;
- the compositor sends configure events with the `resizing` state;
- geometry changes become current only with the client's acked buffer commit.

Button release sends a final configure without `resizing`.

## Minimize

`set_minimized` removes the window from drawing and input but keeps the
xdg_toplevel mapped and keeps its foreign-toplevel entry visible in the Dock.

Restoring a minimized window requires explicit shell/compositor policy. Phase 12
will extend the private shell-control protocol with an identifier-based
`activate_toplevel` request after the minimize state itself is implemented.
The stable identifier already supplied by ext-foreign-toplevel-list is used;
the shell never receives a raw Wayland object belonging to another client.

Activation:

- restores a minimized window;
- raises it;
- gives it keyboard focus;
- does not permit the shell to forge arbitrary client objects.

## Work order

### 12a — state/configure foundation

- window state + saved geometry;
- size constraints;
- configure targets bound to serials;
- activated state follows keyboard focus.

Exit tests: pure state-machine tests + two clients showing activated transitions.

### 12b — maximize/fullscreen

- set/unset maximize;
- set/unset fullscreen;
- restore geometry;
- shell usable-area changes reconfigure maximized windows.

Exit test: QEMU screenshots and configure logs for normal → maximized →
fullscreen → restored.

### 12c — interactive move/resize

- retain button-grab serial;
- validate move/resize requests;
- move geometry;
- resize configure loop and edge rules.

Exit test: QEMU monitor pointer drag moves one window, then resizes it while a
second window proves focus/z-order still work.

### 12d — minimize and Dock restore

- minimized visibility/input policy;
- extend private shell-control with activate_toplevel(identifier);
- Dock click activates/restores and raises the selected window.

Exit test: minimize a window, verify it disappears but remains in the Dock,
click its Dock item, verify it returns focused.

### 12e — closure

- host protocol scenarios;
- MSRV / format / clippy / unit tests;
- Buildroot QEMU window-management test;
- reproducibility re-check;
- Phase 12 report and roadmap PASS only after all evidence is green.

## Non-goals

Not in Phase 12:

- workspaces;
- tiling layouts;
- snap assist;
- animations;
- decorations/theme polish;
- multi-output placement policy.

Those build on this state model later.
