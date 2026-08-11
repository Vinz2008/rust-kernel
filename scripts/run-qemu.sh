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

exec qemu-system-x86_64 \
    -drive "format=raw,file=$IMAGE" \
    "$@"