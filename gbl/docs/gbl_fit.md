# FIT image handling in GBL

This document describes how FIT image is handled by GBL for selecting the
appropriate configuration as per the platform.

[FIT specification][specification] provides a standard format for FIT image. The
present state of the GBL implementation for handling of FIT image supports only
device tree selection and loading via FIT image. FIT image can be flashed either
in DTBO or DTB partition. If FIT image is present in either of these partitions,
the DTBs or DTBOs from all other partitions (including `boot`/`vendor_boot`)
will not be considered. GBL will refuse to boot up if FIT image is present in
both DTBO and DTB partitions.

Each device tree image in the "/images" node should have `flat_dt` type. Each
image binary is expected to be a payload external to the FIT FDT. The sub-node
corresponding to each image should have data-offset and data-size properties.
The data-offset should contain an 8-byte aligned offset to the actual image
binary from the end of the 8-byte aligned FIT FDT. Each image binary should be
8-byte aligned.

The FIT specification [suggests][fit_selection] the use of a compatible
stringlist for selection of the correct configuration. In some cases,
maintaining a stringlist within the bootloader might be cumbersome due to the
multiple configurations required to support minor platform and SKU differences.
GBL provides support for loading a data payload of type "metadata". A metadata
payload can be used by the firmware to match the platform information with the
compatible string maintained for each configuration.

An image of type "metadata" can be added as a sub-node under the "/images" node,
as shown in the example below. If present, "metadata" must be the first image
present in "/images", otherwise it will be ignored by GBL.

```
/ images
  |
  o fdt-0
    |- description = "Image with compressed metadata blob"
    |- data = /incbin/("path/to/data/metadata.bin")
    |- type = "metadata"
    |- data-offset = <0x00000000>
    |- data-size = <0x00000ff8>
    |- align = <0x00000008>
    |...
    |
  o fdt-1
    |- description = "Base devicetree"
    |- data = /incbin/("path/to/data/fdt1.bin")
    |- type = "flat_dt"
    |- data-offset = <0x00000ff8>
    |- data-size = <0x00000ff8>
    |- align = <0x00000008>
    |...
    |
```

The metadata binary can contain information for selecting the configuration
corresponding to the platform on which FIT image is loaded. For e.g., it can
contain SoC ID, board ID, SKU ID etc. Firmware can lookup the corresponding IDs
from the metadata for each configuration and compare against the IDs available
from the platform. GBL doesn't mandate any particular format for the metadata
binary and it can be implementation-defined in accordance with the firmware.

Below is one sample implementation for storing this information within metadata
in FDT format. This builds on top of the example shared in FIT specification.

```
/ o Metadata-tree
  |- description = "Image with compressed metadata blob";
    o soc-ID
    | |
    | o google,kevin
    | |- ID = <google,kevin ID>
    | |
    | o google,kevin-rev15
    | |- ID = <google,kevin-rev15 ID>
    |...
    |
    o SKU
    | |
    | o sku1
    | |- ID = <sku1 ID>
    | |
    | o sku2
    | |- ID = <sku2 ID>
    |...
```

Each configuration should contain a `fdt` property containing the node names for
devicetree images to be loaded for the configuration such that the first node
name refers to the base devicetree and the remaining names refer to the
devicetrees which have to be applied as an overlay to the base devicetree. All
the non-fdt properties are ignored by GBL in the current implementation.

```
/ o FIT FDT
  |
  o images
    |
    o fdt-0
    | | - description = "Image with compressed metadata blob"
    | | - type = "metadata"
    | ...
    |
    o fdt-1
    | | - description = "google,kevin base DT"
    | | - type = "flat_dt"
    | ...
    |
    o fdt-2
    | | - description = "google,kevin rev15 base DT"
    | |...
    |
    o fdt-3
    | | - description = "Overlay DT for sku1 features"
    | |...
    |
    o fdt-4
    | | - description = "Overlay DT for sku2 features"
    | |...
    |
    |...
  o configurations
    |
    o config-1
    | |- description = "Configuration 1"
    | |- compatible = "google,kevin-rev15"
    | |- fdt = "fdt-1"
    | |...
    |
    o config-2
    | |- description = "Configuration 2"
    | |- compatible = "google,kevin-sku1"
    | |- fdt = "fdt-1, fdt-3"
    | |...
    |
    o config-3
    | |- description = "Configuration 3"
    | |- compatible = "google,kevin-sku2"
    | |- fdt = "fdt-1, fdt-4"
    | |...
    |
    o config-4
    | |- description = "Configuration 4"
    | |- compatible = "google,kevin-rev15-sku2"
    | |- fdt = "fdt-2, fdt-4"
    | |...
    |
    |...
```

## Image preparation

The FIT image can be generated using mkimage tool. "-E" flag can be used to keep
the payloads outside FIT image. "-B 8" flag can be used to make the FIT image 8
byte aligned.

[specification]: https://fitspec.osfw.foundation/
[fit_selection]: https://fitspec.osfw.foundation/#select-a-configuration-to-boot
