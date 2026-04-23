# Generic Boot Loader (GBL)

The Generic Bootloader (GBL) monorepo is managed with the `repo` tool and
follows the Android Open Source Project (AOSP) structure.

## Core Directives

- **Architecture:** GBL is a UEFI application encapsulating the Android boot
  process. Its primary goal is to provide a standardized, platform-agnostic
  bootloader core that reduces code duplication across different hardware
  platforms.
- **Platform Support:** GBL supports booting both Android and Fuchsia OSes and
  runs on architectures including x86_64, aarch64, and riscv64.
- **Tech Stack:**
  - **Rust:** Core logic (safety-critical).
  - **C:** Hardware interactions and foundational libraries.
  - **Bazel:** Build system.
  - **Clang:** Compiler (via `prebuilts/`).
- **Monorepo Boundaries:**
  - `bootable/libbootloader/gbl/`: **Core Logic.** The primary development area.
    Most tasks target this path. See `bootable/libbootloader/gbl/GEMINI.md` for
    GBL-specific deep dives, engineering standards, and build commands.
  - `external/`: **Third-party Dependencies.** Contains `libfdt`, `libufdt`,
    AVB, BoringSSL, OpenDICE, and Rust crates (`external/rust/`). Investigate
    these for system-wide context or when tasks involve low-level dependency
    integration.
  - `prebuilts/`: **Toolchains.** Read-only.
