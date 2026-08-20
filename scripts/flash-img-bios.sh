#!/bin/sh
set -eu


if [ "$#" -ne 1 ]; then
    echo "Usage: $0 <device>"
    exit 1
fi


DEVICE="$1"

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

cd ../ && ./build.sh --release
cargo bootimage --release
sudo dd \
    if=target/x86_64-unknown-kernel/release/bootimage-rust-kernel.bin \
    of=$DEVICE \
    bs=4M \
    status=progress \
    conv=fsync