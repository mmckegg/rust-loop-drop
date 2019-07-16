use ::chunk::{Triggerable, OutputValue, SystemTime, ScheduleMode};
use ::midi_connection;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use std::collections::HashMap;

pub use ::scale::Scale;

pub struct BlackboxSlicer {
    last_value: Option<(u8, u8, u8)>,
    offset: Arc<AtomicUsize>,
    velocities: Arc<Mutex<HashMap<u32, u8>>>,
    start: u32,
    midi_channel: u8,
    midi_port: midi_connection::SharedMidiOutputConnection
}

impl BlackboxSlicer {
    pub fn new (midi_port: midi_connection::SharedMidiOutputConnection, midi_channel: u8, start: u32, offset: Arc<AtomicUsize>, velocities: Arc<Mutex<HashMap<u32, u8>>>) -> Self {
        BlackboxSlicer {
            last_value: None,
            start,
            velocities,
            offset,
            midi_channel,
            midi_port
        }
    }
}

impl Triggerable for BlackboxSlicer {
    fn trigger (&mut self, id: u32, value: OutputValue, time: SystemTime) {
        match value {
            OutputValue::Off => (),
            OutputValue::On(_) => {
                let velocities = self.velocities.lock().unwrap();
                let velocity = *velocities.get(&id).unwrap_or(&80);

                let mut offset_value = self.offset.load(Ordering::Relaxed) as u32;
                let mut channel = self.midi_channel;

                let note_id = (36 + ((offset_value % 8) * 8) + id + self.start) as u8;

                // choke last value
                // if let Some((channel, note_id, _)) = self.last_value {
                //     self.midi_port.send_at(&[144 - 1 + channel, note_id, 0], time).unwrap();
                // }
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