use ::chunk::{Triggerable, OutputValue, SystemTime, ScheduleMode, MidiTime, LatchMode};
use ::midi_connection;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use ::scale::Scale;

pub struct BlackboxSlicer {
    last_value: Option<(u8, u8, u8)>,
    bank: Arc<Mutex<BlackboxSlicerBank>>,
    trigger_at: Option<MidiTime>,
    last_pos: MidiTime,
    mode: Arc<Mutex<BlackboxSlicerMode>>,
    midi_port: midi_connection::SharedMidiOutputConnection
}

impl BlackboxSlicer {
    pub fn new (midi_port: midi_connection::SharedMidiOutputConnection, mode: Arc<Mutex<BlackboxSlicerMode>>, bank: Arc<Mutex<BlackboxSlicerBank>>) -> Self {
        BlackboxSlicer {
            last_value: None,
            midi_port,
            bank,
            last_pos: MidiTime::zero(),
            trigger_at: None,
            mode
        }
    }
}

impl Triggerable for BlackboxSlicer {
    fn trigger (&mut self, id: u32, value: OutputValue, time: SystemTime) {
        match value {
            OutputValue::Off => (),
            OutputValue::On(_) => {
                let velocity = 120;
                let mode = self.mode.lock().unwrap();
                let bank = self.bank.lock().unwrap();

                let increment = if let BlackboxSlicerMode::AutoTrigger {rate, length} = *mode {
                    length.ticks() / rate.ticks()
                } else {
                    1
                };

                let (channel, note_id) = match *bank {
                    BlackboxSlicerBank::Single(channel) => (channel, (36 + (id * increment as u32) as u8) % 128), 
                    BlackboxSlicerBank::Split(channel_a, channel_b) => {
                        if id < 4 {
                            (channel_a, (36 + (id * increment as u32) as u8) % 128)
                        } else {
                            (channel_b, (36 + ((id - 4) * increment as u32) as u8) % 128)
                        }
                    }
                };

                // choke last value
                if let Some((channel, note_id, _)) = self.last_value {
                    self.midi_port.send_at(&[144 - 1 + channel, note_id, 0], time).unwrap();
                }
                self.last_value = Some((channel, note_id, velocity));
                self.trigger_at = Some(self.last_pos);

                self.midi_port.send_at(&[144 - 1 + channel, note_id, velocity], time).unwrap();
            }
        }
    }

    fn on_tick (&mut self, time: MidiTime) {
        let mode = self.mode.lock().unwrap();
        self.last_pos = time;
        if let BlackboxSlicerMode::AutoTrigger {rate, length} = *mode {
            if let Some(trigger_at) = self.trigger_at {
                if let Some(mut last_value) = self.last_value {

                    let trigger_pos = time - trigger_at;
                    if trigger_pos < length {
                        let phase = trigger_pos % rate;
                        if phase == MidiTime::zero() {
                            self.midi_port.send(&[144 - 1 + last_value.0, last_value.1, 0]).unwrap();
                            if last_value.1 < 127 {
                                last_value.1 += 1;
                            }
                            self.midi_port.send(&[144 - 1 + last_value.0, last_value.1, last_value.2]).unwrap();
                            self.last_value = Some(last_value);
                        }
                    } else if trigger_pos == length {
                        self.midi_port.send(&[144 - 1 + last_value.0, last_value.1, 0]).unwrap();
                    }
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
            length: MidiTime::from_beats(2), 
            rate: MidiTime::from_measure(1, 4)
        },
        BlackboxSlicerMode::AutoTrigger {
            length: MidiTime::from_beats(1), 
            rate: MidiTime::from_measure(1, 8)
        },
    ];
}

lazy_static! {
    static ref BANKS: [BlackboxSlicerBank; 5] = [
        BlackboxSlicerBank::Split(2, 3),
        BlackboxSlicerBank::Single(2),
        BlackboxSlicerBank::Single(3),
        BlackboxSlicerBank::Single(4),
        BlackboxSlicerBank::Single(5)
    ];
}

#[derive(Debug, Copy, Clone)]
pub enum BlackboxSlicerMode {
    Direct,
    AutoTrigger {length: MidiTime, rate: MidiTime}
}

#[derive(Debug, Copy, Clone)]
pub enum BlackboxSlicerBank {
    Single(u8),
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