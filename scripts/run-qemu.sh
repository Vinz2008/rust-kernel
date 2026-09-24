#!/bin/sh
set -eu

IMAGE="$1"
shift

# for qemu q35, the strict mimimum (at least at the last time tested) is 503 * 1024 + 513
MIN_SIZE=$((1024 * 1024))

SIZE=$(stat -c %s "$IMAGE")

if [ "$SIZE" -lt "$MIN_SIZE" ]; then
    truncate -s "$MIN_SIZE" "$IMAGE"
fi

TEST_DISK="disk.img"

if [ ! -f "$TEST_DISK" ]; then
    qemu-img create -f raw "$TEST_DISK" 64M
fi


# EXE=$(dirname $IMAGE)/rust-kernel
# echo $EXE
# python3 ~/osdev-tools/scripts/gen_syms.py -k $EXE -o kernel.syms

# qemu flag
# -plugin ~/osdev-tools/tools/qemu-profile-plugin/qemu-profile.so,period=10000 \

exec qemu-system-x86_64 \
    -drive "format=raw,file=$IMAGE" \
    -drive format=raw,file=$TEST_DISK,id=disk2 \
    "$@"

# run after python3 ~/osdev-tools/scripts/resolve_profile.py profile kernel.syms > profile-resolved.folded 