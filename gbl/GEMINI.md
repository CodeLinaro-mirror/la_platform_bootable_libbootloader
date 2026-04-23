# Generic Bootloader (GBL)

GBL is a UEFI application encapsulating the Android and Fuchsia boot process,
providing a standardized, platform-agnostic bootloader core.

## Architecture & Design

- **Design:** Connects to EFI APIs through a platform hook model.
- **Goal:** Reduce code duplication in board-specific firmware.
- **Tech Stack:**
  - **Rust:** Core logic (safety-critical).
  - **C:** Hardware interactions and foundational libraries.
  - **Bazel:** Build system.
  - **Clang:** Compiler (via `<monorepo-root>/prebuilts/`).

## Directory Structure Map

- `docs/`: User-facing documentation (for device makers).
- `efi/`: UEFI application binaries.
- `lib*/`: Independent libraries prioritized for modularity.
- `libefi/`: EFI platform hooks connecting core logic to the UEFI environment.
- `libefi_types/`: EFI C declarations and Rust wrappers (No logic).
- `libgbl/`: Core platform-agnostic logic.

## Engineering Standards

### 1. Environment Constraints

- **Target:** Bare-metal UEFI.
- **Rust `no_std`:** Production code MUST declare
  `#![cfg_attr(not(test), no_std)]`. Rely exclusively on `core` and `alloc`.
- **Memory Allocation:** Dynamic allocations (hooked into UEFI) are available
  but highly discouraged in production, where stack allocation or a scratch
  buffer (for large data) is preferred.
- **Memory Allocation in unit tests:** Dynamic allocations are perfectly
  acceptable.

### 2. Testing Constraints

- **Target:** Hosted environment with `std` enabled.
- **Strategy:** Heavily mock UEFI services.
- **Concurrency:** The test runner is multi-threaded. Use `Arc`, `Mutex`,
  `atomics`, or `thread_local!` when testing code that interacts with global
  state (e.g., panic hooks) to prevent race conditions.

### 3. Error Handling

- **No Panics:** Avoid `panic!`, `unwrap()`, or `expect()` in production. These
  cause silent hangs in UEFI.
- **Recoverable Errors:** Return `Result`.
- **Fatal Errors:** Use the `report_error_and_reset` pattern and ensure a
  GBL-specific error tag is provided.

## Building and Testing

The GBL build script is at `bootable/libbootloader/gbl/bazel.sh` relative to the
repo root.

### Building the EFI application

```bash
./bazel.sh run //bootable/libbootloader:gbl_efi_dist
```

### Running unit tests

```bash
./bazel.sh test @gbl//tests
```

## Formatting

Always format code using the following tools before committing.

- **Rust:** `rustfmt <file>`
- **Python:** `pyink <file_or_directory>`
- **C++:** Adhere to Google C++ Style. Use `clang-format -i <file>`.
- **Markdown:** `prettier --write <file>`

## Contribution Guidelines

Follow these standard Android Open Source Project (AOSP) practices for all
contributions.

### 1. Workflow

- **Branching:** Use the `repo` tool.
- **Code Review:** Submit via Gerrit using `repo upload`.

### 2. Commit Messages

All commit messages must strictly adhere to the Android convention. A `Bug:` tag
is **mandatory**.

**Format Example:**

```text
gbl: Resolve boot failure on device XYZ

This change addresses a null pointer dereference in the EFI
protocol handling, which caused a boot hang.

Bug: 123456789
```
