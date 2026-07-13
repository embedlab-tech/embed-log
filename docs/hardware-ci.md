# STM hardware integration CI

`.github/workflows/hardware-integration.yml` separates hardware validation from release publishing.

## Flow

1. A clean GitHub-hosted `ubuntu-latest` job builds and packages the Linux CLI tarball.
2. The `stm-lab` self-hosted runner downloads and installs that exact tarball only for the job.
3. The runner checks its configured serial device, runs the Python hardware harness, and uploads session/support artifacts even on failure.

The workflow never publishes a GitHub Release.

## Runner setup

Give the hardware runner these labels:

```text
self-hosted
Linux
stm-lab
```

Set repository variables:

```text
EMBED_LOG_HARDWARE_UART_PORT=/dev/serial/by-id/<stable-device-id>
EMBED_LOG_HARDWARE_UART_BAUD=115200
```

Use a `/dev/serial/by-id/...` path rather than a changing `ttyACM*` or `ttyUSB*` name.

## Run it

Use **Actions → Hardware integration → Run workflow**. The default command is:

```bash
python -m pytest sdk/python/tests/test_backend_hardware_uart.py -q
```

Override the command when dispatching the workflow to run a different lab harness.

The workflow also runs nightly at 02:00 UTC. It serializes runs with the `stm-lab-hardware` concurrency group, so two jobs cannot operate the same devices at once.

## Security

Do not add pull-request triggers for untrusted forks. A hardware runner executes code against physical devices. Keep it restricted to manual dispatch, scheduled runs, or trusted branch pushes.
