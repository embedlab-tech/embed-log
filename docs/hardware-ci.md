# STM32G0 hardware integration CI

`.github/workflows/hardware-integration.yml` validates the packaged Linux CLI against the dedicated NUCLEO-G070RB/FT4232H rig. It never publishes a GitHub Release.

## Flow

1. A clean GitHub-hosted `ubuntu-latest` job builds and packages the release CLI tarball.
2. The `stm-lab` runner downloads and installs that exact tarball only in `RUNNER_TEMP`.
3. It verifies all four stable UART paths, builds and flashes a pinned pre-provisioned `embed-sandbox` checkout, then runs its independent multi-baud preflight.
4. The STM32G0 pytest starts embed-log with four UART sources (`CONTROL` at 115200, `USART1` at 115200, `USART3` at 460800, and `USART4` at 1000000) and a loopback UDP source. Python automation applies the matching Zephyr-shell baud profiles through `CONTROL`, captures at least 500 deterministic records per data UART, forwards subscribed generator messages over UDP, and verifies source isolation and persisted session files.
5. Captured configuration, server output, logs, and generated session reports are uploaded from `captures/` even when the test fails.

## Runner setup

Give the exclusive physical-lab runner these labels:

```text
self-hosted
Linux
stm-lab
```

Set these repository variables. All serial values must be stable `/dev/serial/by-id/...` paths, never `ttyACM*` or `ttyUSB*` names.

```text
EMBED_LOG_STM32G0_CONTROL_PORT=/dev/serial/by-id/usb-STMicroelectronics_STM32_STLink_0669FF485552787187184556-if02
EMBED_LOG_STM32G0_USART1_PORT=/dev/serial/by-id/usb-FTDI_Quad_RS232-HS-if03-port0
EMBED_LOG_STM32G0_USART3_PORT=/dev/serial/by-id/usb-FTDI_Quad_RS232-HS-if02-port0
EMBED_LOG_STM32G0_USART4_PORT=/dev/serial/by-id/usb-FTDI_Quad_RS232-HS-if00-port0
EMBED_LOG_STM32G0_SANDBOX_DIR=/absolute/path/to/embed-sandbox
EMBED_LOG_STM32G0_FIRMWARE_REVISION=<full-pinned-embed-sandbox-commit-sha>
```

`EMBED_LOG_STM32G0_SANDBOX_DIR` must be a clean (including no untracked files), pre-provisioned checkout at exactly `EMBED_LOG_STM32G0_FIRMWARE_REVISION`, with `just`, the Zephyr toolchain, and the local OpenOCD installation already available. The workflow rejects a dirty or incorrectly pinned checkout before flashing. The runner user also needs access to the ST-LINK and serial devices; see the sandbox handoff for its udev rule.

## Run it

Use **Actions → Hardware integration → Run workflow**. The default command is:

```bash
python -m pytest sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py -q
```

The test is guarded by `EMBED_LOG_STM32G0_HARDWARE=1`, which the workflow sets. It restores the generators to stopped/115200 state through `scenario stop`; the sandbox preflight independently restores all data UARTs to 115200 as well.

The workflow also runs nightly at 02:00 UTC. Its `stm-lab-hardware` concurrency group serializes all runs that use the physical rig.

## Security

Do not add pull-request triggers for untrusted forks. A hardware runner executes code against physical devices. Keep it restricted to manual dispatch, scheduled runs, or trusted branch pushes.
