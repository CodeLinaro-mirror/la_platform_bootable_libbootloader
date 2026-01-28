# GBL Licensing Mechanisms

GBL uses mechanisms built into the Bazel build in order to verify licensing and
generate the combined `LICENSE` output file. As opposed to manually tracking
licenses, this provides automated enforcement that all software has license
attribution and that we're providing up-to-date license texts.

## Requirements

1. Each build target must provide an `applicable_licenses` field, either
   directly in the build target:

   ```
   rust_library(
       name = "foo",
       applicable_licenses = [":foo_license"],
       ...
   )
   ```

   or at the package level via `default_applicable_licenses`:

   ```
   package(
       default_applicable_licenses = [":foo_license"]
   )
   ```

2. The `license()` target used as `applicable_licenses` must provide:
   1. `package_name` which is globally unique

      Note: a `license()` target is unique to a package - even if two different
      dependencies use the same license kind and text, they should have unique
      `license()` declarations to ensure we are properly tracking and licensing
      all dependencies individually.

   2. `license_text` pointing to the file containing the full licensing text

   3. `license_kinds` with one or more license kinds

   For standard Android packages, prefer to use our `generate_license()` rule
   for declaring `license()` targets. This will automatically determine the
   correct `license_kinds` based on the Android `MODULE_LICENSE_*` marker files,
   making it less prone to human error or getting out-of-date.

## Procedure

The `merged_license()` Bazel rule produces a single `LICENSE` file containing
the merged (de-duplicated) license contents of the given target. We run this on
the GBL UEFI application targets to pick up all licenses of anything that
affects these binaries.

If this rule detects any of the expectations have been violated e.g. missing
license text or unsupported license kind, it will fail the build.

Host-only code that has no impact on the produced binaries, such as unittests,
does not need to be included in these licensing mechanisms.
