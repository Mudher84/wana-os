# Wana OS — Final validation plan

This file is intentionally execution-only. Implementation may continue before
this point; evidence collection starts only after the source line is frozen.

## Freeze point

1. Stop implementation changes on `work/prestable-store-recovery`.
2. Record the exact HEAD SHA.
3. Run `make final-validation-test` on that exact source revision.

The target performs, in order:

- release-verifier self-test;
- normal Rust/source checks;
- Rust 1.88 MSRV tests;
- headless Wayland protocol scenarios;
- Buildroot defconfig round-trip;
- full image construction and manifest generation;
- standalone UEFI kernel boot;
- the complete Stable runtime gate, including native desktop, installer,
  permissions/security, hardware matrix, audio, Bluetooth, Wine and Waydroid;
- Phase 35 power/session lifecycle;
- Phase 36 A/B update application, trial boot and confirmation;
- Phase 37 first-boot credential setup, next-boot login and in-session relock;
- Phase 38 supervised Chrony/time diagnostics;
- Phase 39 privacy-bounded diagnostics/support bundle.

## Android full-session evidence

The reproducible release image intentionally does not contain Android OTA
images. After the base final validation is green, provision a dedicated test disk
with the official Waydroid system/vendor images and install one deterministic
test APK. Write its package id (for example `org.example.test`) as the only
line of `/var/lib/wana/android-test-package` inside that test disk, then run:

```
make android-session-boot-test \
  ANDROID_TEST_DISK=/path/to/provisioned.img \
  ANDROID_TEST_PACKAGE=org.example.test
```

The gate requires both `WANA_ANDROID_SESSION_READY` and a successful
`WANA_ANDROID_APK_LAUNCH` marker before accepting the full Android session.
This evidence must not be folded into the reproducible release image.

## Stable reproducibility and publication

The local final-validation target is not a substitute for the two-build Stable
workflow. On the same frozen commit, run `.github/workflows/stable.yml` with
version `0.1.0` and `publish=false` first.

The workflow must prove:

- candidate A passes all Stable runtime gates;
- independent candidate B rebuilds with ccache disabled;
- manifests, artifact sets and SHA-256 values are identical.

Only after those gates are green may the same stable workflow be run with
`publish=true`. Phase reports move from IN PROGRESS to PASS only when their
actual evidence/run IDs are recorded.
