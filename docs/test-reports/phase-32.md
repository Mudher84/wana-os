# Phase 32 — Bluetooth and wireless connectivity

Status: **IN PROGRESS**

## Implemented

- BlueZ with Classic, LE, RFCOMM, BNEP, HID/HOG and audio integration.
- Common USB Bluetooth kernel drivers and Intel/Realtek/MediaTek firmware.
- A supervised root `bluetoothd` service on the system D-Bus.
- `wana-bluetooth` for status, scanning, pairing, connect/disconnect and removal.
- Persistent WPA2/WPA3 management through a supervised `wpa_supplicant`.
- The global WPA control socket is group-owned by `wana`; the desktop user can
  manage Wi-Fi without gaining root privileges or direct read access to stored
  credentials.
- `wana-wifi` supports interface discovery, scan, connect, list, disconnect and
  forget operations; `dhcpcd` supplies IPv4/IPv6 address configuration.
- Kernel support and firmware are included for common Intel IWLWIFI, Realtek
  RTW88/RTW89, MediaTek MT7921, Atheros ath10k and Broadcom brcmfmac hardware.
- Final BlueZ and networking gates are staged but deliberately not executed yet.

## Exit gate

Final validation must prove the service graph and Wi-Fi/BlueZ tooling in the
Buildroot image, then perform real-device checks on representative wireless and
Bluetooth hardware where available.

## Evidence

Pending final consolidated validation. No PASS is claimed yet.
