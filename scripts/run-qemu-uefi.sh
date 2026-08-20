#!/bin/sh
set -eu

cp /usr/share/edk2/x64/OVMF_VARS.4m.fd /tmp/OVMF_VARS.fd
qemu-system-x86_64 -enable-kvm -cpu host -m 2G -drive if=pflash,format=raw,readonly=on,file=/usr/share/edk2/x64/OVMF_CODE.4m.fd -drive if=pflash,format=raw,file=/tmp/OVMF_VARS.fd -drive format=raw,file=target/rust-kernel-uefi.img -serial stdio