MEMORY { LOADER (rwx) : ORIGIN = 0, LENGTH = 0x10000 }

ENTRY(loader_main)

SECTIONS
{
  .entry : { KEEP(*(.entry)) } > LOADER
  .text : { *(.text .text.*) } > LOADER
  .rodata : { *(.rodata .rodata.*) } > LOADER
  .data : { *(.data .data.*) *(.bss .bss.* COMMON) } > LOADER
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.*) }
}

ASSERT(ADDR(.entry) == 0 && SIZEOF(.entry) > 0, "loader_main must come first");
