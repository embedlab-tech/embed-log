uv run utils/inject_log_demo.py --inject SENSOR_A 5001 --inject SENSOR_B 5002 --inject SENSOR_C 5003 --interval 5 --source demo

uv run utils/udp_log_simulator.py --port 6000 --port 6001 --port 6003

uv run backend/server.py --source SENSOR_A udp:6000 --source SENSOR_B udp:6001 --source SENSOR_C udp:6002 --inject SENSOR_A 5001 --inject SENSOR_B 5002 --inject SENSOR_C 5003 --tab "Simulated Devices" SENSOR_A SENSOR_B --tab "Other sensor" SENSOR_C --host 0.0.0.0 --ws-port 8080 --ws-ui frontend/index.html --log-dir logs/ -v
