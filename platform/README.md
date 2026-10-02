# platform/: Buildroot BR2_EXTERNAL tree

Wana keeps distribution-specific Buildroot configuration outside the pinned
Buildroot source tree.

| Path | Contents |
|------|----------|
| `buildroot.env` | Pinned Buildroot version and commit |
| `external.desc`, `external.mk`, `Config.in` | BR2_EXTERNAL glue (external name `WANA`) |
| `configs/wana_x86_64_defconfig` | Canonical x86_64 Wana configuration |
| `board/x86_64/` | Kernel fragment, A/B disk/GRUB layout, users, udev policy and image hooks |
| `package/` | Buildroot packages for Wana Rust components and compatibility integration |

The image contains Wana packages for init, DRM/render/input/compositor/text,
plus the platform integration used by D-Bus, networking, audio, Bluetooth,
authentication, power, updates, Wine and Waydroid.

The defconfig is kept in canonical `savedefconfig` form. `make config-check`
fails if symbols are silently dropped or the checked-in defconfig no longer
round-trips.

Edit configuration with:

```sh
make br-menuconfig
make savedefconfig
```

The installed disk is GPT with an EFI System Partition, fixed-size A/B system
roots and a persistent Data partition placed last so it can expand to the
actual installed device.
