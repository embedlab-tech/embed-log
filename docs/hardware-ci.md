# STM32G0 hardware integration CI

The regular [CI workflow](../.github/workflows/ci.yml) includes the STM32G0 hardware integration job. There is no separate hardware workflow.

## Flow

1. CI runs its normal format, CLI build, Rust/unit, package, and UI test jobs on self-hosted runners.
2. `package-cli-linux` builds and uploads the release CLI tarball.
3. `backend-hardware-tests` (shown as **STM32G0 hardware integration**) runs on the exclusive `stm-lab` runner, downloads that exact tarball, and installs it only in `.tooling/bin` for the job.
4. It checks all four stable UART paths and runs the mixed-baud pytest against the connected, pre-flashed rig.
5. Captured configuration, server output, logs, and session reports under `captures/stm32g0/` are uploaded even if the test fails.

The hardware job has a global `stm-lab-hardware` concurrency group so physical-rig runs from different branches cannot overlap. Although the runner does not need a custom label, it is dedicated to this repository's physical rig.

## Runner setup

The dedicated `embed-log-runner` is registered with these labels:

```text
self-hosted
Linux
```

The job defaults to the verified paths on this runner. Set these optional repository variables only when the rig uses different stable `/dev/serial/by-id/...` paths; never use `ttyACM*` or `ttyUSB*` names.

```text
EMBED_LOG_STM32G0_CONTROL_PORT=/dev/serial/by-id/usb-STMicroelectronics_STM32_STLink_0669FF485552787187184556-if02
EMBED_LOG_STM32G0_USART1_PORT=/dev/serial/by-id/usb-FTDI_Quad_RS232-HS-if03-port0
EMBED_LOG_STM32G0_USART3_PORT=/dev/serial/by-id/usb-FTDI_Quad_RS232-HS-if02-port0
EMBED_LOG_STM32G0_USART4_PORT=/dev/serial/by-id/usb-FTDI_Quad_RS232-HS-if00-port0
```

The runner user needs access to the ST-LINK and serial devices. The board must already be flashed and running; this job needs no firmware checkout, Zephyr toolchain, OpenOCD, or `just` installation.

## Test behavior

The job runs:

```bash
python -m pytest sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py -q
```

The test configures `CONTROL` and `USART1` at 115200, `USART3` at 460800, and `USART4` at 1000000. It captures at least 500 deterministic records per generator, forwards records through a loopback UDP source, then stops generators and restores data UARTs to 115200. Because CI enables `EMBED_LOG_STM32G0_HARDWARE=1`, a missing configured UART path fails the job instead of being reported as a passing skip.

## Security

The CI workflow runs code on self-hosted runners. Do not add pull-request triggers for untrusted forks to the hardware job.
