//! Studio Replay Playback Driver
//!
//! Feeds recorded `.mcpr` packet streams into the client's packet processing
//! loop at user-controlled playback speeds (slow-motion, 1x, fast-forward, pause).

use super::mcpr::{McprReader, Metadata, TimestampedPacket};
use std::{io, path::Path, time::Duration};

/// Playback controller driving recorded packets according to a virtual timeline.
pub struct ReplayDriver {
    pub metadata: Metadata,
    reader: McprReader,
    cursor_ms: f64,
    speed: f64,
    paused: bool,
    looping: bool,
    finished: bool,
    next_packet_cache: Option<TimestampedPacket>,
}

impl ReplayDriver {
    /// Open an `.mcpr` recording archive for replay playback.
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut reader = McprReader::open(path)?;
        let metadata = reader.metadata.clone();
        let next_packet_cache = reader.next_packet()?;

        Ok(Self {
            metadata,
            reader,
            cursor_ms: 0.0,
            speed: 1.0,
            paused: false,
            looping: false,
            finished: false,
            next_packet_cache,
        })
    }

    /// Current timeline position in milliseconds.
    pub fn position_ms(&self) -> u32 {
        self.cursor_ms as u32
    }

    /// Total recording duration in milliseconds.
    pub fn duration_ms(&self) -> u32 {
        self.metadata.duration
    }

    /// Total number of indexed packets in the recording stream.
    pub fn packet_count(&self) -> usize {
        self.reader.total_packets()
    }

    /// Current playback progress as a fraction from 0.0 to 1.0.
    pub fn progress(&self) -> f32 {
        if self.metadata.duration == 0 {
            0.0
        } else {
            (self.cursor_ms / self.metadata.duration as f64).clamp(0.0, 1.0) as f32
        }
    }

    /// Current playback speed multiplier (e.g. 0.25, 1.0, 2.0).
    pub fn speed(&self) -> f64 {
        self.speed
    }

    /// Set playback speed multiplier. Clamped between 0.01x and 20.0x.
    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed.clamp(0.01, 20.0);
    }

    /// Check if playback is paused.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Set paused state.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Toggle paused state.
    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    /// Set whether replay should loop when reaching the end.
    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    /// Check if playback reached the end of the recording.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Advance virtual time by `delta` and collect all packets scheduled up to the new time.
    pub fn advance(&mut self, delta: Duration) -> io::Result<Vec<TimestampedPacket>> {
        if self.paused || self.finished {
            return Ok(Vec::new());
        }

        let delta_ms = delta.as_secs_f64() * 1000.0 * self.speed;
        self.cursor_ms += delta_ms;

        let mut ready = Vec::new();
        let target_ms = self.cursor_ms as u32;

        loop {
            let Some(pkt) = &self.next_packet_cache else {
                if self.cursor_ms >= self.metadata.duration as f64 {
                    if self.looping {
                        self.seek(0)?;
                    } else {
                        self.finished = true;
                    }
                }
                break;
            };

            if pkt.timestamp_ms <= target_ms {
                ready.push(self.next_packet_cache.take().unwrap());
                self.next_packet_cache = self.reader.next_packet()?;
            } else {
                break;
            }
        }

        Ok(ready)
    }

    /// Seek directly to a specific timestamp in milliseconds.
    pub fn seek(&mut self, target_ms: u32) -> io::Result<()> {
        let clamped_ms = target_ms.min(self.metadata.duration);
        self.cursor_ms = clamped_ms as f64;
        self.finished = false;

        self.reader.seek_to_timestamp(clamped_ms)?;
        self.next_packet_cache = self.reader.next_packet()?;
        Ok(())
    }

    /// Rewind playback to the beginning.
    pub fn restart(&mut self) -> io::Result<()> {
        self.seek(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::mcpr::McprWriter;
    use std::fs;

    #[test]
    fn test_replay_driver_advance_and_speed() {
        let temp_dir =
            std::env::temp_dir().join(format!("replay_driver_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let mcpr_path = temp_dir.join("driver_test.mcpr");

        let meta = Metadata {
            duration: 1000,
            ..Default::default()
        };
        let mut writer = McprWriter::new(mcpr_path.clone(), meta).unwrap();
        writer.write_packet(0, 0x01, &[1]).unwrap();
        writer.write_packet(100, 0x02, &[2]).unwrap();
        writer.write_packet(200, 0x03, &[3]).unwrap();
        writer.write_packet(500, 0x04, &[4]).unwrap();
        writer.finish(1000).unwrap();

        let mut driver = ReplayDriver::open(&mcpr_path).unwrap();
        assert_eq!(driver.position_ms(), 0);
        assert_eq!(driver.duration_ms(), 1000);

        // Advance 50ms at 1.0x -> receives packet at 0ms
        let batch1 = driver.advance(Duration::from_millis(50)).unwrap();
        assert_eq!(batch1.len(), 1);
        assert_eq!(batch1[0].id, 0x01);

        // Advance another 100ms (now 150ms) -> receives packet at 100ms
        let batch2 = driver.advance(Duration::from_millis(100)).unwrap();
        assert_eq!(batch2.len(), 1);
        assert_eq!(batch2[0].id, 0x02);

        // Paused -> advance produces nothing
        driver.set_paused(true);
        let batch_paused = driver.advance(Duration::from_millis(200)).unwrap();
        assert!(batch_paused.is_empty());
        driver.set_paused(false);

        // Speed 2.0x -> advance 100ms real time advances 200ms virtual time (150ms -> 350ms)
        driver.set_speed(2.0);
        let batch3 = driver.advance(Duration::from_millis(100)).unwrap();
        assert_eq!(batch3.len(), 1);
        assert_eq!(batch3[0].id, 0x03);

        // Seek back to 0
        driver.seek(0).unwrap();
        assert_eq!(driver.position_ms(), 0);

        let _ = fs::remove_file(mcpr_path);
        let _ = fs::remove_dir(temp_dir);
    }
}
