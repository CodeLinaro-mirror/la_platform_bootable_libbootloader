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

"""Macros for instantiating QEMU tests."""

def qemu_test(
        name,
        gbl,
        disk,
        test_script = None,
        gbl_launcher = "@gbl//tests/qemu/gbl_launcher:gbl_launcher_aarch64",
        qemu = "@vmm//:qemu/x86_64-linux-gnu/bin/gbl-qemu-system-aarch64",
        bios = "@vmm//:qemu/x86_64-linux-gnu/usr/share/qemu/edk2-aarch64-code.fd",
        vhost_device_vsock = "@vhost_device_vsock",
        out = None,
        timeout = None,
        env = {},
        **kwargs):
    """Instantiates a QEMU test genrule.

    Args:
        name: The name of the test target.
        gbl: Target label for the GBL binary.
        disk: Target label (or list of target labels) for disk images to attach.
        test_script: Target label for the test Python script.
        gbl_launcher: Target label for the GBL launcher application.
        qemu: Target label for the QEMU binary.
        bios: Target label for the BIOS/UEFI firmware.
        vhost_device_vsock: Target label for vhost_device_vsock. Can be None.
        out: Name of the serial log output file. If None, defaults to
             `"{name}_log.txt"`.
        timeout: Optional Starlark integer timeout in seconds.
        env: Optional Starlark dictionary mapping custom environment variable
             names to their values.
        **kwargs: General rule arguments passed to the underlying genrule.
    """

    # Determine the default output result filename.
    if out == None:
        out = "{name}_log.txt".format(name = name)

    # Collect input files required by the test launcher.
    srcs = [
        "@gbl//tests/qemu:qemu_launcher.py",
        gbl_launcher,
        gbl,
        qemu,
        bios,
    ]

    # Normalize disk targets into a list and add to inputs.
    disks = []
    if disk != None:
        if type(disk) == "string":
            disks = [disk]
        elif type(disk) == "list" or type(disk) == "tuple":
            disks = list(disk)
    srcs.extend(disks)

    # Gather executable host tool dependencies.
    tools = []
    if test_script != None:
        tools.append(test_script)
    if vhost_device_vsock != None:
        tools.append(vhost_device_vsock)

    # Assemble the base Python test launcher command.
    cmd = [
        "python3 $(location @gbl//tests/qemu:qemu_launcher.py)",
        "$(location " + gbl_launcher + ")",
        "$(location " + gbl + ")",
        "--test_name " + name,
        "--qemu $(location " + qemu + ")",
        "--bios $(location " + bios + ")",
        "--log_output $@",
    ]

    # Append disks
    for d in disks:
        cmd.append("--disk $(location " + d + ")")

    # If userspace vsock is enabled, add vsock device.
    if vhost_device_vsock != None:
        cmd.append("--vhost_device_vsock $(location " + vhost_device_vsock + ")")

    # If userspace test script is enabled, add test script.
    if test_script != None:
        cmd.append("--test_script $(location " + test_script + ")")

    # If timeout is set, add timeout.
    if timeout != None:
        cmd.append("--timeout " + str(timeout))

    # Add custom environment variables as inline shell assignments.
    env_prefix = []
    if env != None:
        for k, v in env.items():
            env_prefix.append('{}="{}"'.format(k, str(v).replace('"', '\\"')))

    # Instantiate the underlying Bazel genrule.
    native.genrule(
        name = name,
        srcs = srcs,
        outs = [out],
        cmd = " ".join(env_prefix + cmd),
        tools = tools,
        **kwargs
    )
