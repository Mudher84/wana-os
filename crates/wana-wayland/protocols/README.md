# Protocols kept in this repository

Protocols that are not part of libwayland or wayland-protocols, pinned here
([decision 0003](../../../docs/decisions/0003-shell-surfaces.md)). `build.rs`
generates their tables like the others.

| File | Source | Version | sha256 |
|---|---|---|---|
| `wlr-layer-shell-unstable-v1.xml` | [swaywm/wlr-protocols](https://github.com/swaywm/wlr-protocols/blob/master/unstable/wlr-layer-shell-unstable-v1.xml) (archived mirror of gitlab.freedesktop.org/wlroots/wlr-protocols) | 4 | `996b8c66e1c2276c443336d4cb174634e8e5948823779042f244930525b79cc2` |
| `wana-shell-control-v1.xml` | Wana OS project-owned protocol | 1 | `14dacee956f5a9d9b507cea4785510753cad20e632bb7bf019a8259b9b5fbf44` |

The file is unchanged from the source. Its license (HPND-style, permissive)
is in its `<copyright>` block.


`wana-shell-control-v1` is intentionally private: the compositor advertises it only on the connection it creates for `wana-shell`. It is not a public application API.
