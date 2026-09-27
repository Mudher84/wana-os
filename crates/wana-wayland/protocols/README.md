# Protocols kept in this repository

Protocols that are not part of libwayland or wayland-protocols, pinned here
([decision 0003](../../../docs/decisions/0003-shell-surfaces.md)). `build.rs`
generates their tables like the others.

| File | Source | Version | sha256 |
|---|---|---|---|
| `wlr-layer-shell-unstable-v1.xml` | [swaywm/wlr-protocols](https://github.com/swaywm/wlr-protocols/blob/master/unstable/wlr-layer-shell-unstable-v1.xml) (archived mirror of gitlab.freedesktop.org/wlroots/wlr-protocols) | 4 | `996b8c66e1c2276c443336d4cb174634e8e5948823779042f244930525b79cc2` |

The file is unchanged from the source. Its license (HPND-style, permissive)
is in its `<copyright>` block.

| `wana-shell-control-v1.xml` | Wana OS project | 1 | project-owned; GPL-2.0-or-later |

`wana-shell-control-v1` is the private compositor-to-shell control protocol from [decision 0004](../../../docs/decisions/0004-shell-control.md). It is not advertised to public clients.
