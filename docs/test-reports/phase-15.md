# Phase 15 — Networking

Status: **IN PROGRESS**

## Implemented

- Native network discovery through kernel + udev.
- Interface state, carrier, MAC and wireless classification.
- VirtIO, e1000 and rtl8139 validation paths.
- A supervised `wpa_supplicant` backend with WPA2/WPA3 and nl80211.
- A group-owned control socket lets the unprivileged `wana` session manage
  wireless networks without direct access to the root-owned credential file.
- `wana-wifi` supports interface discovery, scan, connect, list, disconnect
  and forget operations.
- A supervised `dhcpcd` service configures wired and wireless IPv4/IPv6.
- Common Intel, Realtek, MediaTek, Atheros and Broadcom Wi-Fi kernel drivers and
  firmware are present in the target configuration.

## Exit gate

The final validation must run `make network-boot-test`, the alternate NIC
hardware matrix and the managed Wi-Fi service checks. Real Wi-Fi evidence is
recorded separately when representative hardware is available.

## Evidence

Implementation and the final gates exist. Consolidated Buildroot/runtime
evidence is deliberately deferred to the final validation pass.
