# user memory (lower half)
0x0000_0000_0000_0000 - 0x0000_0000_000b_8000 : empty
0x0000_0000_000b_8000 - 0x0000_0000_000b_8fa0 : vga buffer
0x0000_0000_000b_8fa0 - 0x0000_0000_0020_0000 : empty
0x0000_0000_0020_0000 - ...                    : userspace ELF LOAD segments
0x0000_0000_4000_0000 - 0x0000_0000_8000_0000 : user heap, 1 GiB
0x0000_0000_8000_0000 - 0x0000_7fff_ffef_e000 : empty
0x0000_7fff_ffef_e000 - 0x0000_7fff_ffef_f000 : guard page (4 KiB, unmapped)
0x0000_7fff_ffef_f000 - 0x0000_7fff_ffff_f000 : user process stack (1MiB)
0x0000_7fff_ffff_f000 - 0x0000_8000_0000_0000 : empty

# non-canonical hole

0x0000_8000_0000_0000 - 0xffff_8000_0000_0000 : invalid, can't use

# kernel memory (higher half)
0xffff_8000_0000_0000 - 0xffff_9000_0000_0000 : kernel process stacks
0xffff_9000_0000_0000 - 0xffff_9000_00a0_0000 : kernel heap (10 MiB)
0xffff_9000_00a0_0000 - 0xffff_ffff_8000_0000 : empty
0xffff_ffff_8000_0000 - 0xffff_ffff_ffff_ffff : kernel image