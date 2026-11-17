ENTRY(Reset_Handler)
SECTIONS { . = 0x00001000; .text : { *(.text*) } .bss : { *(.bss*) *(COMMON) } }
