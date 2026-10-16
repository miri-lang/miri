#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

# Report whether this machine is in the state a publication run needs, and
# exit non-zero when it is not:
#
#   bench/machine/check.sh [pinned-core]
#
# On Linux it checks the frequency governor, turbo/boost, the pinned core's
# hyper-thread sibling and whether the core is isolated. bench/run.py makes the
# same checks before every cell of a publication run. On macOS nothing can be
# enforced: it records the power source, low-power mode and thermal state, and
# always reports the machine unfit for publication.

set -u
core="${1:-2}"
fail=0

report() { printf '%-28s %s\n' "$1" "$2"; }

if [ "$(uname -s)" = "Darwin" ]; then
    report "power source" "$(pmset -g batt | head -n 1 | sed "s/.*'\(.*\)'.*/\1/")"
    report "low power mode" "$(pmset -g | awk '/lowpowermode/ {print $2}')"
    report "thermal state" "$(pmset -g therm | tail -n 1)"
    report "publication" "refused: macOS offers no governor, turbo or core isolation control"
    exit 1
fi

governor="$(cat /sys/devices/system/cpu/cpu${core}/cpufreq/scaling_governor 2>/dev/null || echo unknown)"
report "governor (core ${core})" "$governor"
[ "$governor" = "performance" ] || fail=1

if [ -f /sys/devices/system/cpu/intel_pstate/no_turbo ]; then
    turbo_off="$(cat /sys/devices/system/cpu/intel_pstate/no_turbo)"
elif [ -f /sys/devices/system/cpu/cpufreq/boost ]; then
    turbo_off=$(( 1 - $(cat /sys/devices/system/cpu/cpufreq/boost) ))
else
    turbo_off=unknown
fi
report "turbo off" "$turbo_off"
[ "$turbo_off" = "1" ] || fail=1

siblings="$(cat /sys/devices/system/cpu/cpu${core}/topology/thread_siblings_list 2>/dev/null || echo unknown)"
report "core ${core} thread siblings" "$siblings"
[ "$siblings" = "$core" ] || fail=1

isolated="$(cat /sys/devices/system/cpu/isolated 2>/dev/null)"
report "isolated cores" "${isolated:-none}"
report "ASLR" "$(cat /proc/sys/kernel/randomize_va_space)"
report "transparent huge pages" "$(cat /sys/kernel/mm/transparent_hugepage/enabled)"

if [ "$fail" -ne 0 ]; then
    report "publication" "refused"
    exit 1
fi
report "publication" "ready"
