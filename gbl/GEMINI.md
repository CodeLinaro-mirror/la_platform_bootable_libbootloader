# Gemini Guidelines for GBL (Generic Bootloader)

This document provides guidelines for interacting with the GBL codebase using the Gemini CLI.

## Project Overview

GBL, or Generic Bootloader, is a UEFI application designed to standardize the boot process for Android and Fuchsia operating systems. It encapsulates common boot logic, reducing duplication in board-specific firmware.

The primary source code for GBL is located within the `bootable/libbootloader/gbl` directory. Third-party dependencies and prebuilts are generally located in higher-level `external/` or `prebuilts/` directories and should not be modified directly.

## Code Structure

*   `docs/`: Contains user-facing documentation, primarily for device makers who are writing UEFI firmware to run GBL.
*   `efi/`: Contains the final UEFI application binary.
*   `lib*/`: These directories contain independent libraries. The primary goal is code modularity rather than widespread reuse.
*   `libefi/`: Contains the glue to connect the platform-agnostic libgbl to EFI-specific platform hooks.
*   `libefi_types/`: EFI C type declarations and Rust wrappers. This should only contain types and wrappers, not logic.
*   `libgbl/`: This is the core, platform-agnostic logic for GBL. It uses a platform hook model and does not call any EFI APIs directly.

## Building and Testing

Refer to the `README.md` file in this directory for the most up-to-date instructions on how to build and test the project. The commands should be run from the root of the Android UEFI manifest checkout (`../../../` from this directory).

## Code Style and Formatting

Before committing, please ensure your code is formatted according to the project's standards.

*   **Rust:** Use `rustfmt` to format Rust code.
    ```bash
    cargo fmt --all
    ```

*   **Python:** Use the `black` formatter.
    ```bash
    black .
    ```

*   **C++:** Adhere to the Google C++ Style Guide. Use `clang-format` to format C++ code.
    ```bash
    clang-format -i $(find . -name "*.h" -o -name "*.cpp")
    ```

## Contribution Workflow

This project follows standard Android contribution practices.

*   **Development:** Use the `repo` tool for managing branches.
*   **Code Reviews:** Submit changes for review via Gerrit with `repo upload`.
*   **Commit Messages:** Format commit messages according to the standard Android convention. All commit messages must include a `Bug:` tag. A `Test:` tag is not required.

    **Example:**
    ```
    gbl: Resolve boot failure on device XYZ

    This change addresses a null pointer dereference in the EFI
    protocol handling, which caused a boot hang.

    Bug: b/123456789
    ```
