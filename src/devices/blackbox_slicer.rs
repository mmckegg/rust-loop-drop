use ::chunk::{Triggerable, OutputValue, SystemTime, ScheduleMode, MidiTime, LatchMode};
use ::midi_connection;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::collections::HashMap;

pub use ::scale::Scale;

pub struct BlackboxSlicer {
    last_value: HashMap<usize, (u8, u8, u8)>,
    trigger_at: HashMap<usize, MidiTime>,
    bank: Arc<Mutex<BlackboxSlicerBank>>,
    last_pos: MidiTime,
    mode: Arc<Mutex<BlackboxSlicerMode>>,
    midi_port: midi_connection::SharedMidiOutputConnection
}

impl BlackboxSlicer {
    pub fn new (midi_port: midi_connection::SharedMidiOutputConnection, mode: Arc<Mutex<BlackboxSlicerMode>>, bank: Arc<Mutex<BlackboxSlicerBank>>) -> Self {
        BlackboxSlicer {
            last_value: HashMap::new(),
            trigger_at: HashMap::new(),
            midi_port,
            bank,
            last_pos: MidiTime::zero(),
            mode
        }
    }
}

impl Triggerable for BlackboxSlicer {
    fn trigger (&mut self, id: u32, value: OutputValue, time: SystemTime) {

        let mode = self.mode.lock().unwrap();

        let increment = if let BlackboxSlicerMode::AutoTrigger {rate, ..} = *mode {
            24 / rate.ticks()
        } else {
            1
        };

        let bank = self.bank.lock().unwrap();
        let (channel, note_id, slicer_id) = match *bank {
            BlackboxSlicerBank::Single(channel, offset) => (channel, (36 + ((id + offset) * increment as u32) as u8) % 128, 0), 
            BlackboxSlicerBank::Split(channel_a, channel_b) => {
                if id < 4 {
                    (channel_a, (36 + (id * increment as u32) as u8) % 128, 0)
                } else {
                    (channel_b, (36 + ((id - 4) * increment as u32) as u8) % 128, 1)
                }
            }
        };

        match value {
            OutputValue::Off => {
                if *mode == BlackboxSlicerMode::Direct {
                    self.midi_port.send_at(&[144 - 1 + channel, note_id, 0], time).unwrap();
                }
            },
            OutputValue::On(_) => {
                let velocity = 120;
                // choke last value
                if let Some((channel, note_id, _)) = self.last_value.get(&slicer_id) {
                    self.midi_port.send_at(&[144 - 1 + channel, *note_id, 0], time).unwrap();
                }
                self.last_value.insert(slicer_id, (channel, note_id, velocity));
                self.trigger_at.insert(slicer_id, self.last_pos);

                self.midi_port.send_at(&[144 - 1 + channel, note_id, velocity], time).unwrap();
            }
        }
    }

    fn on_tick (&mut self, time: MidiTime) {
        let mode = self.mode.lock().unwrap();
        self.last_pos = time;
        if let BlackboxSlicerMode::AutoTrigger {rate, length} = *mode {
            for (slicer_id, trigger_at) in &self.trigger_at {
                if let Some(mut last_value) = self.last_value.remove(&slicer_id) {
                    let trigger_pos = time - *trigger_at;
                    if trigger_pos < length {
                        let phase = trigger_pos % rate;
                        if phase == MidiTime::zero() {
                            self.midi_port.send(&[144 - 1 + last_value.0, last_value.1, 0]).unwrap();
                            if last_value.1 < 127 {
                                last_value.1 += 1;
                            }
                            let velocity = (last_value.2 as f32 * 0.7) as u8;
                            self.midi_port.send(&[144 - 1 + last_value.0, last_value.1, velocity]).unwrap();
                        }
                    } else if trigger_pos == length {
                        self.midi_port.send(&[144 - 1 + last_value.0, last_value.1, 0]).unwrap();
                    }
                    self.last_value.insert(*slicer_id, last_value);
                }
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

pub struct BlackboxSlicerModeChooser {
    value: Arc<Mutex<BlackboxSlicerMode>>
}

pub struct BlackboxSlicerBankChooser {
    value: Arc<Mutex<BlackboxSlicerBank>>
}

lazy_static! {
    static ref MODES: [BlackboxSlicerMode; 3] = [
        BlackboxSlicerMode::Direct,
        BlackboxSlicerMode::AutoTrigger {
            length: MidiTime::from_beats(1), 
            rate: MidiTime::from_measure(1, 4)
        },
        BlackboxSlicerMode::AutoTrigger {
            length: MidiTime::from_measure(1, 2), 
            rate: MidiTime::from_measure(1, 4)
        },
    ];
}

lazy_static! {
    static ref BANKS: [BlackboxSlicerBank; 5] = [
        BlackboxSlicerBank::Split(2, 3),
        BlackboxSlicerBank::Single(2, 0),
        BlackboxSlicerBank::Single(2, 8),
        BlackboxSlicerBank::Single(3, 0),
        BlackboxSlicerBank::Single(3, 8)
    ];
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum BlackboxSlicerMode {
    Direct,
    AutoTrigger {length: MidiTime, rate: MidiTime}
}

#[derive(Debug, Copy, Clone)]
pub enum BlackboxSlicerBank {
    Single(u8, u32),
    Split(u8, u8)
}

impl BlackboxSlicerModeChooser {
    pub fn new (value: Arc<Mutex<BlackboxSlicerMode>>) -> Self {
        Self {
            value
        }
    }

    pub fn default_value () -> Arc<Mutex<BlackboxSlicerMode>> {
        Arc::new(Mutex::new(MODES[0]))
    }
}

impl Triggerable for BlackboxSlicerModeChooser {
    fn trigger (&mut self, id: u32, value: OutputValue, _at: SystemTime) {
        match value {
            OutputValue::Off => {},
            OutputValue::On(velocity) => {
                let mode = MODES[id as usize % MODES.len()];
                let mut value = self.value.lock().unwrap();
                *value = mode;
            }
        }
    }

    fn latch_mode (&self) -> LatchMode { LatchMode::LatchSingle }
}

impl BlackboxSlicerBankChooser {
    pub fn new (value: Arc<Mutex<BlackboxSlicerBank>>) -> Self {
        Self {
            value
        }
    }

    pub fn default_value () -> Arc<Mutex<BlackboxSlicerBank>> {
        Arc::new(Mutex::new(BANKS[0]))
    }
}

impl Triggerable for BlackboxSlicerBankChooser {
    fn trigger (&mut self, id: u32, value: OutputValue, _at: SystemTime) {
        match value {
            OutputValue::Off => {},
            OutputValue::On(velocity) => {
                let mode = BANKS[id as usize % BANKS.len()];
                let mut value = self.value.lock().unwrap();
                *value = mode;
            }
        }
    }

    fn latch_mode (&self) -> LatchMode { LatchMode::LatchSingle }
}