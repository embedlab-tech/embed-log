# cortex-m4-fault.elf

Tiny Cortex-M4 firmware with DWARF debug information, used by the crash
symbolization tests (`src/symbols.rs`, `src/net/crash.rs`). `process_sample`
inlines `scale_sample`, which dereferences a NULL pointer at `0x102a`; the
return address into `sensor_task` is `0x104b` (Thumb bit set).

Rebuild (clang and lld):

```bash
mkdir -p src && cp cortex-m4-fault.c src/sensor.c
clang --target=thumbv7em-none-eabi -mcpu=cortex-m4 -mthumb -g -O1 -ffreestanding -nostdlib \
  -fuse-ld=lld -Wl,-T,cortex-m4-fault.ld -fdebug-prefix-map=$PWD=/work/firmware \
  -o cortex-m4-fault.elf src/sensor.c
```
