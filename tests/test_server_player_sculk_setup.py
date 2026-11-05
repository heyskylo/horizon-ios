"""The walking harness must wait out setup vibrations before measuring movement."""
import unittest

from tools.test_server_player_sculk_parity import wait_for_quiet_sensor


class SensorTrace:
    def __init__(self, phases):
        self.phases = phases
        self.tick = 0

    def wait(self, ticks):
        self.tick += ticks

    def phase(self, _host):
        phase = next(value for start, value in reversed(self.phases) if start <= self.tick)
        return f'minecraft:sculk_sensor[sculk_sensor_phase={phase}]'


class QuietSensorTests(unittest.TestCase):
    def test_waits_out_an_in_flight_vibration(self):
        # VanillaSculkOracle's sensor_step trace: selected at 2, arrives at 5,
        # active for 30 ticks, cooldown for 10. Tick 0 alone looks ready.
        sensor = SensorTrace([(0, 'inactive'), (5, 'active'), (35, 'cooldown'), (45, 'inactive')])
        wait_for_quiet_sensor(sensor, sensor.phase)
        self.assertEqual(sensor.tick, 54)

    def test_requires_a_fresh_quiet_window_after_each_vibration(self):
        sensor = SensorTrace([(0, 'active'), (30, 'cooldown'), (40, 'inactive'),
                              (48, 'active'), (78, 'cooldown'), (88, 'inactive')])
        wait_for_quiet_sensor(sensor, sensor.phase)
        self.assertEqual(sensor.tick, 97)

    def test_does_not_wait_forever_for_a_noisy_setup(self):
        sensor = SensorTrace([(0, 'active')])
        with self.assertRaisesRegex(AssertionError, 'sensor did not settle during setup'):
            wait_for_quiet_sensor(sensor, sensor.phase)
        self.assertEqual(sensor.tick, 100)


if __name__ == '__main__':
    unittest.main()
