# GBL Resources

This doc provides some resources to help with implementing GBL support on a
device.

Official GBL information and requirements can be found at
https://source.android.com/docs/core/architecture/bootloader/generic-bootloader.
This documentation focuses more on specific coding details, i.e. the "how"
rather than the "what".

## Where to get this

### Prebuilt GBL

When using a prebuilt GBL image from the Android builders, these resources will
be available in a separate archive:

- `gbl-img-{build_number}.zip`: GBL binaries
- `gbl-resources-{build_number}.zip`: these docs

The GBL builds can be found at
https://ci.android.com/builds/branches/aosp_uefi-gbl-mainline/grid. Artifacts
for a specific build number are at
`https://ci.android.com/builds/submitted/{build_number}/gbl_efi_dist_and_test/latest`.

### Building GBL from source

When building GBL yourself, these documents will be available directly in the
GBL source tree.

## Contents

### Docs

The `docs/` directory contains markdown files describing GBLs features,
behaviors, and all the UEFI protocols it uses.

### Headers

The `include/` directory contains C headers that provide definitions for the
various UEFI types and constants used in GBL.

While these files are intended to be generally usable, they may need some
modifications to port to a particular bootloader codebase, e.g. fixing up
include paths or changing some type definitions for a particular device.

Devices also are not required to use these headers; they are free to implement
the UEFI types another way, as long as they adhere to the ABI required by UEFI
and the GBL-specific protocols.
