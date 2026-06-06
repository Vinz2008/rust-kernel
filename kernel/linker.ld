ENTRY(_start)

SECTIONS
{
    . = 0xffffffff80000000;

    .text : {
        *(.text .text.*)
    }

    . = ALIGN(0x1000);

    .rodata : {
        *(.rodata .rodata.*)
    }

    . = ALIGN(0x1000);

    .data.rel.ro : {
        *(.data.rel.ro .data.rel.ro.*)
        *(.got .got.*)
    }

    . = ALIGN(0x1000);

    .data : {
        *(.data .data.*)
    }

    .bss (NOLOAD) : {
        *(COMMON)
        *(.bss .bss.*)
    }

    /DISCARD/ : {
        *(.eh_frame)
        *(.eh_frame_hdr)
        *(.debug_*)
        *(.comment)
    }
}