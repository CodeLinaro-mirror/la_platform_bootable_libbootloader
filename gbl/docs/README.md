# GBL Documentation

This directory contains documentation for the Generic Bootloader (GBL).

## Table of Contents

### General Documentation

- [UEFI Overview](./efi_integration.md): Lists the UEFI protocols and interfaces
  used by GBL.
- [A/B Boot Flow](./gbl_ab_boot_flow.md): Explains GBL's A/B boot logic.
- [Android Boot Flow](./gbl_android_flow.md): Explains GBL's Android boot flow.
- [Buffer Usage](./gbl_buffer_usage.md): Discusses memory buffer management.
- [Fastboot](./gbl_fastboot.md): Describes GBL's Fastboot implementation.
- [FIT Image Handling](./gbl_fit.md): Details how FIT images are processed.
- [Partition Management](./partitions.md): Details how GBL handles disk
  partitions.

### Custom UEFI Protocols

- [Android Verified Boot Protocol (`GBL_EFI_AVB_PROTOCOL`)](./gbl_efi_avb_protocol.md)
- [AVF Protocol (`GBL_EFI_AVF_PROTOCOL`)](./gbl_efi_avf_protocol.md)
- [Boot Control Protocol (`GBL_EFI_BOOT_CONTROL_PROTOCOL`)](./gbl_efi_boot_control_protocol.md)
- [Boot Memory Protocol (`GBL_EFI_BOOT_MEMORY_PROTOCOL`)](./gbl_efi_boot_memory_protocol.md)
- [Debug Protocol (`GBL_EFI_DEBUG_PROTOCOL`)](./gbl_efi_debug_protocol.md)
- [Fastboot Protocol (`GBL_EFI_FASTBOOT_PROTOCOL`)](./gbl_efi_fastboot_protocol.md)
- [Fastboot Transport Protocol (`GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL`)](./gbl_efi_fastboot_transport_protocol.md)
- [OS Configuration Protocol (`GBL_EFI_OS_CONFIGURATION_PROTOCOL`)](./gbl_efi_os_configuration_protocol.md)

## Formatting

GBL documentation uses [Prettier][prettier] to ensure consistent formatting
across GBL source and documentation files. Example command:

```sh
prettier --write ./bootable/libbootloader/gbl/**/*.md
```

[prettier]: https://prettier.io/
