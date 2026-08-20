#!/bin/sh
set -eu

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

cd ../
./scripts/build-uefi.sh

EFI="$PWD/bootloader/target/x86_64-unknown-uefi/release/bootloader-uefi.efi"
IMG="$PWD/target/rust-kernel-uefi.img"

truncate -s 128M "$IMG"
sgdisk --clear "$IMG"
sgdisk \
    --new=1:2048:0 \
    --typecode=1:EF00 \
    --change-name=1:"EFI System" \
    "$IMG"

LOOP=$(sudo losetup --find --show --partscan "$IMG")

sudo mkfs.fat -F 32 "${LOOP}p1"

MNT=$(mktemp -d)
sudo mount "${LOOP}p1" "$MNT"

sudo mkdir -p "$MNT/EFI/BOOT"
sudo cp "$EFI" "$MNT/EFI/BOOT/BOOTX64.EFI"

sync

sudo umount "$MNT"
sudo losetup -d "$LOOP"
rmdir "$MNT"