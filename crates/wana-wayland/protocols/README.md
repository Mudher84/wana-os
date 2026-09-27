# Protocols kept in this repository

Protocols that are not part of libwayland or wayland-protocols, pinned here
([decision 0003](../../../docs/decisions/0003-shell-surfaces.md)). `build.rs`
generates their tables like the others.

| File | Source | Version | sha256 |
|---|---|---|---|
| `wlr-layer-shell-unstable-v1.xml` | [swaywm/wlr-protocols](https://github.com/swaywm/wlr-protocols/blob/master/unstable/wlr-layer-shell-unstable-v1.xml) (archived mirror of gitlab.freedesktop.org/wlroots/wlr-protocols) | 4 | `996b8c66e1c2276c443336d4cb174634e8e5948823779042f244930525b79cc2` |

The file is unchanged from the source. Its license (HPND-style, permissive)
is in its `<copyright>` block.


## Wana shell-control

`wana-shell-control-v1.xml` is a Wana-owned private protocol introduced by
[decision 0004](../../../docs/decisions/0004-shell-control.md).

It is intentionally not an upstream Wayland protocol:

- it is advertised only to the compositor-launched private `wana-shell` client;
- public applications must never see or bind it;
- v1 carries semantic shell actions, not raw input;
- changes require a decision update, protocol tests and a version bump when
  compatibility would otherwise be broken.

The compositor remains the authority for global shortcuts. The shell remains
the authority for shell UI.
