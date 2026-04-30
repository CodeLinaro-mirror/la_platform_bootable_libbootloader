# Copyright (C) 2026 The Android Open Source Project
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#       http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""Rules and macros for building synthetic GPT disk images."""

def gpt_gen(name, disk_size, partitions, entries_count = 128, **kwargs):
    """Generates a GPT disk image using the gpt_gen tool.

    Args:
        name: The name of the rule and the output target. The output file
              will be `name + ".img"`.
        disk_size: The size of the disk image (e.g., "1", "1M", "1G").
        partitions: A dictionary mapping partition names to their configuration.
                    The configuration is a dictionary supporting the following keys:
                    - "size": Required. The size of the partition (e.g., "100MiB").
                    - "file": Optional. A target label  containing the image file
                              to initialize the partition with.
        entries_count: The number of partition entries in the GPT table. Defaults to 128.
        **kwargs: General rule arguments passed to the underlying genrule.
    """
    cmd = ["$(location //tools/gpt_gen:gpt_gen)"]
    cmd.append("$@")  # Output file
    cmd.append(disk_size)

    srcs = []
    for part_name, part_info in partitions.items():
        size = part_info.get("size")
        file_label = part_info.get("file")

        part_str = part_name + "," + size
        if file_label:
            part_str += ",$(location " + file_label + ")"
            srcs.append(file_label)
        else:
            part_str += ","

        cmd.append("--partition")
        cmd.append(part_str)

    cmd.append("--entries_count")
    cmd.append(str(entries_count))

    native.genrule(
        name = name,
        srcs = srcs,
        outs = [name + ".img"],
        cmd = " ".join(cmd),
        tools = ["//tools/gpt_gen:gpt_gen"],
        **kwargs
    )
