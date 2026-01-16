#!/usr/bin/env python3

# Copyright (C) 2026 The Android Open Source Project
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""Generates a single LICENSE file we can distribute with GBL binaries.

The input is a JSON license map produced by the Bazel build, output is a text
file listing licenses sequentially, with duplicates removed.

All `license()` declarations used must declare a unique `package_name` and a
valid `license_text` that points to the full license file.
"""

import argparse
import collections
import json
import pathlib


def load_license_map(json_path: pathlib.Path) -> dict[str, str]:
    """Loads a license_map.json file and all license texts.

    Args:
        json_path: path to the input JSON file as formatted by rules_license
                   `write_licenses_info()`.

    Returns:
        A dict of {package_name : license_text}.

    Raises:
        KeyError if the JSON has a missing or invalid key.
        FileNotFoundError if the indicated license file doesn't exist.
    """
    with open(json_path, "r") as f:
        license_map = json.load(f)

    # We use the rules_license `licenses_used()` rule to create the license map JSON.
    # It provides both a target->licenses mapping for each individual build target, as
    # well as a license->targets map containing the license details.
    #
    # Example output for a single libfdt_c target:
    #
    # [
    #   {
    #     "top_level_target": "@@gbl+//efi:all_platforms",
    #     "dependencies": [
    #       {
    #         "target_under_license": "@@gbl++_repo_rules2+libfdt_c//:libfdt_c",
    #         "licenses": [
    #           "@@gbl++_repo_rules2+libfdt_c//:license"
    #         ]
    #       },
    #     ],
    #     "licenses": [
    #       {
    #         "label": "@@gbl++_repo_rules2+libfdt_c//:license",
    #         "rule": "@@gbl++_repo_rules2+libfdt_c//:license",
    #         "license_kinds": [
    #           {
    #             "target": "@rules_license+//licenses/spdx:BSD-2-Clause",
    #             "name": "BSD-2-Clause",
    #             "long_name": "BSD-2-Clause",
    #             "conditions": []
    #           }
    #         ],
    #         "copyright_notice": "",
    #         "package_name": "libfdt",
    #         "package_url": "",
    #         "package_version": "",
    #         "license_text": "external/gbl++_repo_rules2+libfdt_c/BSD-2-Clause",
    #         "used_by": [
    #           "@@gbl++_repo_rules2+libfdt_c//:libfdt_c"
    #         ]
    #       },
    #     ]
    #   }
    # ]
    license_by_package = {}
    for target_entry in license_map:
        for license_info in target_entry.get("licenses", []):
            # We require all our license declarations provide a unique package_name,
            # which allows us to provide a human-readable name for each license rather
            # than trying to use Bazel labels.
            package = license_info.get("package_name")
            if not package:
                label = license_info.get("label", "<unknown>")
                raise KeyError(f"License '{label}' must provide a package_name")
            if package in license_by_package:
                # Two different licenses are claiming to be for the same package. A
                # package can have multiple license_kinds (e.g. dual-licensed) but the
                # top-level license describes the exact licensing to apply to a single
                # package so must be unique to that package.
                raise KeyError(f"Multiple licenses found for package '{package}'")

            # "license_text" is the relative path to the file which Bazel has populated
            # since we declared all these files as inputs.
            text_path = pathlib.Path(license_info["license_text"])
            if not text_path.is_file():
                raise FileNotFoundError(
                    f"license_text for '{package}' points to non-existent file '{text_path}'"
                )
            # Strip leading and trailing whitespace to allow de-dup in these trivial
            # cases. We could be a little more thorough here (e.g. check for licenses
            # that just use different indentation) but it's better to err on the safe
            # side and make sure we don't combine licenses that are actually distinct.
            text = text_path.read_text(encoding="utf-8", errors="replace").strip()

            license_by_package[package] = text

    return license_by_package


def write_merged_license(license_map: dict, out_path: pathlib.Path):
    """Writes the merged license file.

    Args:
        license_map: a {package_name : license_text} dict to write.
        out_path: the file path to write the final license text to.
    """
    # First pull out the top-level GBL license, we'll always list this separately first
    # for clarity even if the license text gets duplicated in our dependencies.
    gbl_license_text = license_map.pop("GBL", None)
    if not gbl_license_text:
        raise ValueError("GBL license not found in license map")

    # De-duplicate dependency licenses which match exactly.
    packages_by_license_text = collections.defaultdict(set)
    for package, license_text in license_map.items():
        packages_by_license_text[license_text].add(package)
    # End up with a de-duped {(packages): text} dict.
    license_text_by_packages = {
        tuple(sorted(packages)): text
        for text, packages in packages_by_license_text.items()
    }

    # Delimiters to use to separate sections.
    delimiter = ("=" * 80) + "\n"

    with open(out_path, "w", encoding="utf-8") as f:
        # GBL license first.
        f.write(delimiter)
        f.write("Generic Bootloader (GBL) License\n")
        f.write(delimiter)
        f.write(gbl_license_text)
        f.write("\n\n")

        # Dependency list (names only) alphabetical for easy reference.
        f.write(delimiter)
        f.write("GBL Third-Party Software\n")
        f.write(delimiter)
        for package in sorted(license_map.keys()):
            f.write(f"{package}\n")
        f.write("\n")

        # Dependency license texts, de-duplicated.
        for packages in sorted(license_text_by_packages.keys()):
            f.write(delimiter)
            f.write(f"GBL Third-Party License: {', '.join(packages)}\n")
            f.write(delimiter)
            f.write(license_text_by_packages[packages])
            f.write("\n\n")


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)

    parser.add_argument(
        "license_map", type=pathlib.Path, help="Path to the license map JSON input"
    )
    parser.add_argument(
        "out", type=pathlib.Path, help="Path to the output file to write"
    )

    return parser.parse_args()


def main() -> None:
    args = _parse_args()

    license_map = load_license_map(args.license_map)
    write_merged_license(license_map, args.out)


if __name__ == "__main__":
    main()
