/* Tiny Cortex-M firmware used as a symbolization fixture. */
volatile int *volatile sensor_reg;
volatile int sink;

static inline __attribute__((always_inline)) int scale_sample(int raw)
{
	return *sensor_reg * raw; /* faults when sensor_reg is NULL */
}

__attribute__((noinline)) int process_sample(int raw)
{
	int value = scale_sample(raw);
	sink = value;
	return value;
}

__attribute__((noinline)) void sensor_task(void)
{
	for (int i = 0; i < 4; i++) {
		process_sample(i);
	}
	sink = 0;
}

void Reset_Handler(void)
{
	sensor_task();
	for (;;) {
	}
}
