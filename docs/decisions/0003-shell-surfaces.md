# Decision 0003: How the shell draws its surfaces, and who may

- Status: **Proposed** (2026-09-26); waiting for the project owner
- Date: 2026-09-26
- Context: [ARCHITECTURE.md §7](../ARCHITECTURE.md) makes `wana-shell` a separate process: a privileged Wayland client
  that draws the desktop background, the top bar, the dock and the launcher, so a shell crash cannot take down the
  compositor or the applications. With the compositor (Phase 10) and the text stack (decision 0002) in place,
  Phase 11 needs two things decided before any code:
  1. the protocol the shell uses for surfaces that are not windows: anchored to screen edges, above or below
     windows, reserving space;
  2. how the compositor makes sure only the shell gets those powers.

## Facts (pinned versions, measured 2026-09-26)

- wayland-protocols 1.45 (Buildroot) has no protocol for shell surfaces. It does have protocols the shell will use
  later:
  - `ext-foreign-toplevel-list-v1`: the list of open windows, for the dock;
  - `ext-session-lock-v1`: the lock screen;
  - `ext-workspace-v1`: workspaces;
  - `xdg-activation-v1`: focus handover when an app starts;
  - `security-context-v1`: tags the connections of sandboxed apps.
- `wlr-layer-shell-unstable-v1` is the de facto standard for shell surfaces. sway, Hyprland, niri, KDE KWin and
  COSMIC implement it, and third-party panels, docks and launchers (waybar, fuzzel, …) use it.
  - Upstream is gitlab.freedesktop.org (version 5). The GitHub mirror (`swaywm/wlr-protocols`, archived) has
    version 4, sha256 `996b8c66…5b79cc2`, 18 KB, under an HPND-style permissive license. Version 5 only adds
    `set_exclusive_edge`.
  - Its model:
    - four layers: background, bottom, top and overlay (windows sit between bottom and top);
    - anchors to screen edges, margins, and an exclusive zone that windows must avoid;
    - keyboard interactivity: none, exclusive or on-demand;
    - popups through `xdg_popup`.
- libwayland-server has `wl_display_set_global_filter`, which decides per client which globals it can see. A
  client that cannot see a global cannot bind it.
- Wana's scanner already generates the tables from any protocol XML, so a protocol costs us one XML file either way.

## Options

### A. wlr-layer-shell (the pinned v4 XML kept in the repository) (recommended)

- Wana implements a known, well-specified model instead of inventing one. The hard parts (exclusive zones,
  keyboard interactivity, stacking) are specified and have been exercised by many clients.
- Third-party shells and panels could run on Wana later, if the owner ever wants that.
- Cost: the interface names are `zwlr_*` ("unstable"). That is only a name; the protocol has been stable in
  practice for years.

### B. Our own `wana-shell-v1`

- Only what Wana's shell needs, named as ours.
- Cost: we would design the same semantics again (layers, anchors, zones, focus), with no existing clients to test
  against, for no user-visible gain.

### C. Shell inside the compositor process

- No protocol at all. Rejected: it breaks the isolation the architecture requires. A shell bug would take down every
  window.

## Privilege model (for A or B)

- The compositor starts the shell itself. It creates the connection with a `socketpair` and `wl_client_create`,
  passes the other end to the shell as `WAYLAND_SOCKET`, and marks that client as the shell. The shell never
  connects through the public socket, so no other process can impersonate it by timing or by name.
- `wl_display_set_global_filter`: the layer-shell global, and later the foreign-toplevel list, session lock and
  workspaces, are visible only to the shell client. Every other client does not see them at all, so it cannot bind
  them. That is stronger than refusing the bind.
- If the shell crashes, the compositor logs it, keeps the windows, and restarts it with a limit, as `wana-init`
  does for services.
- `security-context-v1` (sandboxed apps) comes later, with the permission broker (Phase 22-23).

## Phase 11 plan (each step with its own test)

1. **Protocol and privilege:**
   - `wlr-layer-shell` tables generated from the pinned XML;
   - the shell's connection made by the compositor;
   - the global filter.

   Test: the shell client sees `zwlr_layer_shell_v1`. `wayland-info` on the public socket does not see it.
2. **Layer surfaces:**
   - the configure handshake, anchors, margins and exclusive zones;
   - stacking: background, then bottom, then windows, then top, then overlay.

   Test: a background and a top bar, and a window placed below the bar's exclusive zone (screenshot pixels).
3. **wana-shell MVP:**
   - background;
   - top bar with the time and a status area, laid out RTL with wana-text;
   - a launcher (overlay, keyboard exclusive) that lists apps and starts one;
   - the dock through `ext-foreign-toplevel-list`.

   Test: QEMU opens the launcher, starts a client, and its window appears. A shell crash keeps the window and the
   shell comes back.
4. **Report + reproducibility re-check.**

## Recommendation

**Option A with the privilege model.** It keeps Wana's policy in Wana's code (which client is the shell, what it may
see) and uses a proven surface model instead of a new one.
