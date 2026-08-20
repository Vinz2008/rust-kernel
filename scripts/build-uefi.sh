#!/bin/sh
set -eu

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

cd ../
# TODO : reenable release mode ?
#./build.sh --release
./build.sh
KERNEL="$(pwd)/target/x86_64-unknown-kernel/release/rust-kernel"
KERNEL_MANIFEST="$(pwd)/Cargo.toml"
cd bootloader

KERNEL="$KERNEL" \
KERNEL_MANIFEST="$KERNEL_MANIFEST" \
cargo build \
    --release \
    --target x86_64-unknown-uefi \
    --bin bootloader-uefi \
    --features "binary map_physical_memory sse uefi"