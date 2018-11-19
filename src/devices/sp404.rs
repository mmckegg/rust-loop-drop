use ::chunk::{Triggerable, OutputValue, SystemTime, ScheduleMode};
use ::midi_connection;
use std::sync::{Arc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

pub use ::scale::Scale;

pub struct SP404 {
    last_value: Option<(u8, u8, u8)>,
    offset: Arc<AtomicUsize>,
    midi_channel: u8,
    midi_port: midi_connection::SharedMidiOutputConnection
}

impl SP404 {
    pub fn new (midi_port: midi_connection::SharedMidiOutputConnection, midi_channel: u8, offset: Arc<AtomicUsize>) -> Self {
        SP404 {
            last_value: None,
            offset,
            midi_channel,
            midi_port
        }
    }
}

impl Triggerable for SP404 {
    fn trigger (&mut self, id: u32, value: OutputValue, time: SystemTime) {
        match value {
            OutputValue::Off => (),
            OutputValue::On(_) => {
                let mut offset_value = self.offset.load(Ordering::Relaxed);
                let mut channel = if offset_value < 5 {
                    self.midi_channel
                } else {
                    self.midi_channel + 1
                };

                let velocity = 127;
                let note_id = (47 + ((offset_value % 5) * 12) + (id as usize)) as u8;

                // choke last value
                if let Some((channel, note_id, _)) = self.last_value {
                    self.midi_port.send_at(&[128 - 1 + channel, note_id, 0], time).unwrap();
                }
                self.last_value = Some((channel, note_id, velocity));

                self.midi_port.send_at(&[144 - 1 + channel, note_id, velocity], time).unwrap();
            }
        }
    }

    fn schedule_mode (&self) -> ScheduleMode {
        ScheduleMode::Percussion
    }

    fn latency_offset (&self) -> Option<Duration> {
        Some(Duration::from_millis(5))
    }
}