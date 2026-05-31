# Sample commands

```bash
# Start server from YAML config (recommended)
embed-log run --config embed-log.demo.yml

# Or via Python module (useful during development)
python3 -m backend.server run --config embed-log.demo.yml

# Start server with legacy CLI flags (inline source definitions)
embed-log run \
  --source SENSOR_A udp:6000 \
  --source SENSOR_B udp:6001 \
  --source SENSOR_C udp:6002 \
  --inject SENSOR_A 5001 \
  --inject SENSOR_B 5002 \
  --inject SENSOR_C 5003 \
  --tab "DevA" SENSOR_A SENSOR_B \
  --tab "DevB" SENSOR_C \
  --host 127.0.0.1 \
  --ws-port 8080 \
  --log-dir logs/

# Validate a config file
embed-log validate --config embed-log.demo.yml

# Generate a config interactively
embed-log create-config

# List recorded sessions
embed-log sessions list

# Export a session as HTML
embed-log sessions export <session-id>

# Show session information
embed-log sessions info <session-id>

# Print session logs with search
embed-log sessions logs <session-id> --grep "timeout"

# Send demo markers
python3 utils/inject_log_demo.py \
  --inject SENSOR_A 5001 \
  --inject SENSOR_B 5002 \
  --inject SENSOR_C 5003 \
  --interval 5 \
  --source demo

# Generate demo UDP traffic (deterministic, for UI tests)
python3 utils/deterministic_demo_traffic.py \
  --udp SENSOR_A=127.0.0.1:6000 \
  --udp SENSOR_B=127.0.0.1:6001 \
  --udp SENSOR_C=127.0.0.1:6002 \
  --tick-ms 100 \
  --cycles 0

# Generate random demo UDP traffic
python3 utils/udp_log_simulator.py \
  --target 127.0.0.1:6000 \
  --target 127.0.0.1:6001 \
  --target 127.0.0.1:6002

# Merge log files into a static HTML viewer (offline)
python3 utils/merge_logs.py \
  --tab "UART" "Device A" logs/DEVICE_A.log \
               "Device B" logs/DEVICE_B.log

# Check version and environment
embed-log version

# Run backend tests
python3 -m unittest discover -s tests -v

# Run UI tests
cd tests-ui && npm test
```
