# STM32G0 hardware integration CI

`.github/workflows/hardware-integration.yml` runs the complete local validation flow on the dedicated NUCLEO-G070RB/FT4232H runner. It never publishes a GitHub Release or uses a GitHub-hosted build job.

## Flow

1. The `stm-lab` self-hosted runner checks out the revision and builds the release CLI locally.
2. It runs Rust CLI/core unit tests and the Python SDK/backend integration suite against that local build.
3. It installs the locked `tests-ui` dependencies and Playwright Chromium before running Node UI unit, Playwright end-to-end, and Playwright regression tests.
4. It verifies all four stable UART paths on the pre-flashed, connected rig.
5. The STM32G0 pytest starts embed-log with four UART sources (`CONTROL` at 115200, `USART1` at 115200, `USART3` at 460800, and `USART4` at 1000000) and a loopback UDP source. Python automation applies the matching Zephyr-shell baud profiles through `CONTROL`, captures at least 500 deterministic records per data UART, forwards subscribed generator messages over UDP, and verifies source isolation and persisted session files.
6. Captured configuration, server output, logs, session reports, and Playwright reports are uploaded even when a test fails.

## Runner setup

Give the exclusive physical-lab runner these labels:

```text
self-hosted
Linux
stm-lab
```

The workflow defaults to the verified paths on this runner. Set these optional repository variables only if the lab rig uses different stable `/dev/serial/by-id/...` paths; never use `ttyACM*` or `ttyUSB*` names.

```text
EMBED_LOG_STM32G0_CONTROL_PORT=/dev/serial/by-id/usb-STMicroelectronics_STM32_STLink_0669FF485552787187184556-if02
EMBED_LOG_STM32G0_USART1_PORT=/dev/serial/by-id/usb-FTDI_Quad_RS232-HS-if03-port0
EMBED_LOG_STM32G0_USART3_PORT=/dev/serial/by-id/usb-FTDI_Quad_RS232-HS-if02-port0
EMBED_LOG_STM32G0_USART4_PORT=/dev/serial/by-id/usb-FTDI_Quad_RS232-HS-if00-port0
```

The runner user needs access to the ST-LINK and serial devices; see the sandbox handoff for its udev rule. No firmware checkout, Zephyr toolchain, OpenOCD, or `just` installation is needed by this workflow: it assumes the board is already flashed and running.

## Run it

Use **Actions → STM lab validation → Run workflow**. The complete local flow runs before the hardware command. The default command is:

```bash
python -m pytest sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py -q
```

The test is guarded by `EMBED_LOG_STM32G0_HARDWARE=1`, which the workflow sets. It stops the generators and restores the USART3/USART4 firmware baud rates to 115200 during teardown.

The workflow runs on pushes to the trusted `release-mvp` branch, manual dispatch, and nightly at 02:00 UTC. Its `stm-lab-hardware` concurrency group serializes all runs that use the physical rig.

## Security

Do not add pull-request triggers for untrusted forks. A hardware runner executes code against physical devices. Keep it restricted to manual dispatch, scheduled runs, or trusted branch pushes.
