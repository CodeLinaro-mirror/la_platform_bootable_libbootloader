# Gemini Guidelines for GBL (Generic Bootloader)

This document provides guidelines for interacting with the GBL codebase using
the Gemini CLI.

## Project Overview

GBL, or Generic Bootloader, is a UEFI application designed to standardize the
boot process for Android and Fuchsia operating systems. It encapsulates common
boot logic, reducing duplication in board-specific firmware.

The primary source code for GBL is located within the
`bootable/libbootloader/gbl` directory. Third-party dependencies and prebuilts
are generally located in higher-level `external/` or `prebuilts/` directories
and should not be modified directly.

## Code Structure

- `docs/`: Contains user-facing documentation, primarily for device makers who
  are writing UEFI firmware to run GBL.
- `efi/`: Contains the final UEFI application binary.
- `lib*/`: These directories contain independent libraries. The primary goal is
  code modularity rather than widespread reuse.
- `libefi/`: Contains the glue to connect the platform-agnostic libgbl to
  EFI-specific platform hooks.
- `libefi_types/`: EFI C type declarations and Rust wrappers. This should only
  contain types and wrappers, not logic.
- `libgbl/`: This is the core, platform-agnostic logic for GBL. It uses a
  platform hook model and does not call any EFI APIs directly.

## Building and Testing

For quick reference, the most common commands are:

- **Build all EFI applications:**
  ```bash
  ./bazel.sh run //bootable/libbootloader:gbl_efi_dist
  ```
- **Run all unittests:**
  ```bash
  ./bazel.sh test @gbl//tests
  ```

## Engineering Standards

- **Environment:** GBL targets a bare-metal UEFI environment. The code must be
  `#![cfg_attr(not(test), no_std)]`. Rely on `core` and `alloc`.
- **Testing:** Unit tests run in a hosted environment with `std` enabled. Tests
  heavily mock UEFI services.
- **Global State:** Because the test runner is multi-threaded, take care when
  testing code that interacts with global states (like panic hooks). Use `Arc`,
  `Mutex`, or `thread_local!` to avoid race conditions.
- **Error Handling:**
  - **Production:** Avoid `panic!`, `unwrap()`, or `expect()` in production
    code. These can cause silent hangs in a UEFI environment.
  - **Pattern:** Favor the `Result` type for recoverable errors. Use the
    `report_error_and_reset` pattern for fatal failures, ensuring a GBL-specific
    error tag is provided.

## Code Style and Formatting

- **Rust:** Format with `rustfmt`.
- **Python:** Format with `pyink`.
- **C++:** Format with `clang-format` adhering to the Google C++ Style Guide.
- **Markdown:** Format with `prettier`.

## Contribution Workflow

This project follows standard Android contribution practices.

- **Development:** Use the `repo` tool for managing branches.
- **Code Reviews:** Submit changes for review via Gerrit with `repo upload`.
- **Commit Messages:** Format commit messages according to the standard Android
  convention. All commit messages must include a `Bug:` tag. A `Test:` tag is
  not required.

  **Example:**

  ```
  gbl: Resolve boot failure on device XYZ

  This change addresses a null pointer dereference in the EFI
  protocol handling, which caused a boot hang.

  Bug: b/123456789
  ```
