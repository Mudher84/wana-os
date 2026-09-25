# platform/: Buildroot BR2_EXTERNAL tree

| Path | Contents |
|------|----------|
| `buildroot.env` | Pinned Buildroot version and commit |
| `external.desc`, `external.mk`, `Config.in` | BR2_EXTERNAL glue (external name `WANA`) |
| `configs/wana_x86_64_defconfig` | The Wana OS x86_64 configuration |
| `package/` | Buildroot packages for Wana components (none yet) |

The defconfig is kept in canonical `savedefconfig` form. `make config-check`
fails when it is not, which also catches symbols that Buildroot silently
drops (typos, or options removed in a new Buildroot release).

Edit the configuration with `make br-menuconfig`, then `make savedefconfig`.
