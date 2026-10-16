#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

# Put a Linux box into the state bench/machine/publication.toml describes.
# Run as root, once per boot, before a publication run:
#
#   sudo bench/machine/setup-linux.sh [pinned-core]
#
# Sets every core's frequency governor to `performance`, turns turbo/boost off,
# takes the pinned core's hyper-thread sibling offline, and sets transparent
# huge pages to `madvise`. Address-space layout randomisation is left at the
# kernel's default and recorded, as the charter says. For a stricter box, also
# boot with `isolcpus=<core> nohz_full=<core>`; check.sh reports whether the
# core is isolated.

set -eu
core="${1:-2}"

for governor in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor; do
    echo performance > "$governor"
done

if [ -f /sys/devices/system/cpu/intel_pstate/no_turbo ]; then
    echo 1 > /sys/devices/system/cpu/intel_pstate/no_turbo
elif [ -f /sys/devices/system/cpu/cpufreq/boost ]; then
    echo 0 > /sys/devices/system/cpu/cpufreq/boost
fi

siblings="/sys/devices/system/cpu/cpu${core}/topology/thread_siblings_list"
for sibling in $(tr ',' ' ' < "$siblings" | tr '-' ' '); do
    if [ "$sibling" != "$core" ] && [ -f "/sys/devices/system/cpu/cpu${sibling}/online" ]; then
        echo 0 > "/sys/devices/system/cpu/cpu${sibling}/online"
    fi
done

echo madvise > /sys/kernel/mm/transparent_hugepage/enabled
echo "prepared core ${core}; ASLR is $(cat /proc/sys/kernel/randomize_va_space)"
