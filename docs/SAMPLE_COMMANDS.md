# Sample commands

```bash
# Start server from YAML config (recommended)
python3 backend/server.py run --config embed-log.demo.yml

# Start server with legacy CLI flags
python3 backend/server.py \
  --source SENSOR_A udp:6000 \
  --source SENSOR_B udp:6001 \
  --source SENSOR_C udp:6002 \
  --inject SENSOR_A 5001 \
  --inject SENSOR_B 5002 \
  --inject SENSOR_C 5003 \
  --tab "Simulated Devices" SENSOR_A SENSOR_B \
  --tab "Other Sensor" SENSOR_C \
  --host 127.0.0.1 \
  --ws-port 8080 \
  --ws-ui frontend/index.html \
  --log-dir logs/

# Send demo markers
python3 utils/inject_log_demo.py \
  --inject SENSOR_A 5001 \
  --inject SENSOR_B 5002 \
  --inject SENSOR_C 5003 \
  --interval 5 \
  --source demo

# Generate demo UDP traffic
python3 utils/udp_log_simulator.py \
  --target 127.0.0.1:6000 \
  --target 127.0.0.1:6001 \
  --target 127.0.0.1:6002
```