#! /usr/bin/env python

# Copyright 2025, The Android Open Source Project
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

import argparse
import io
import pathlib
import re
import sys

# Haven't found a way to make the regex do all the work for picking out
# individual derive elements. Capturing the chunk in the middle
# and splitting it up manually is the safest, easiest solution.
DERIVE_RE = re.compile(
    r"(?P<leading>\s*)#\[derive\((?P<derives>.+?)\)\](?P<trailing>[^\n]*)\n"
)

# Tuple for immutability and ordering.
# Unfortunately, derive macro ordering can be significant.
# This order is somewhat alphabetical but also groups related traits
# and has not caused compilation issues or runtime bugs so far.
EXPECTED_DERIVES: tuple[str, ...] = (
    "Copy",
    "Clone",
    "Debug",
    "Default",
    "PartialEq",
    "Eq",
    "PartialOrd",
    "Ord",
    "Hash",
    "Immutable",
    "AsBytes",
    "FromBytes",
    "FromZeroes",
    "IntoBytes",
    "KnownLayout",
    "Unaligned",
)


def fix_file(f: io.TextIOWrapper, in_place: bool = False) -> bool:
    linum_to_invalid_derives: dict[int, list[str]] = {}
    all_lines: list[str] = []
    file_dirty = False

    for linum, line in enumerate(f, start=1):
        match = DERIVE_RE.match(line)
        if not match:
            all_lines.append(line)
            continue

        # Filter trailing commas
        derives: set[str] = {
            d.strip() for d in match.groupdict()["derives"].split(",") if d
        }
        invalid_derives: list[str] = [d for d in derives if d not in EXPECTED_DERIVES]
        if invalid_derives:
            linum_to_invalid_derives[linum] = invalid_derives
            continue

        if not linum_to_invalid_derives:
            sorted_derives = ", ".join(ed for ed in EXPECTED_DERIVES if ed in derives)
            leading = match.groupdict()["leading"]
            trailing = match.groupdict()["trailing"]
            new_derive_line = f"{leading}#[derive({sorted_derives})]{trailing}\n"
            all_lines.append(new_derive_line)
            file_dirty |= new_derive_line != line

    if linum_to_invalid_derives:
        print(
            f"Unexpected derives in the following locations: {f.name}", file=sys.stderr
        )
        for linum, bad_derive in linum_to_invalid_derives.items():
            print(f"\t{', '.join(bad_derive)} @ {linum}", file=sys.stderr)
    elif not in_place:
        print("".join(all_lines))
    elif file_dirty:
        f.seek(0)
        f.truncate()
        f.write("".join(all_lines))

    return not bool(linum_to_invalid_derives)


def main() -> bool:
    parser = argparse.ArgumentParser(
        description="Sort derive macros and complain if unexpected ones are found"
    )
    parser.add_argument(
        "-i",
        "--in-place",
        action="store_true",
        help="Write back changes to disk",
    )
    parser.add_argument("file", nargs="+", type=pathlib.Path)

    args = parser.parse_args()

    all_files_good = True
    for p in args.file:
        with open(p, "r+", encoding="UTF-8") as f:
            all_files_good &= fix_file(f, in_place=args.in_place)

    return all_files_good


if __name__ == "__main__":
    sys.exit(0 if main() else 1)
