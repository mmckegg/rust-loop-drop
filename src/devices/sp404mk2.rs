use chunk::{MidiTime, OutputValue, Triggerable};
use midi_connection;
use serde::{Deserialize, Serialize};

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use super::{midi_keys::Offset, SidechainOutput};

const SUB_NOTES: [u8; 7] = [4, 5, 6, 7, 0, 1, 2];

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum OffsetPress {
    None,
    Pressed {
        instant: Instant,
        value: u8,
        count: usize,
    },
}

impl OffsetPress {
    fn value_count(&self, value: u8) -> usize {
        if let OffsetPress::Pressed {
            value: v, count, ..
        } = self
        {
            if v == &value {
                return *count;
            }
        }

        0
    }

    fn get_before(&self, instant: Instant) -> Option<(u8, usize)> {
        if let OffsetPress::Pressed {
            value,
            count,
            instant: i,
        } = self
        {
            if i < &instant {
                return Some((*value, *count));
            }
        }

        None
    }
}

pub struct Sp404Mk2 {
    output: midi_connection::SharedMidiOutputConnection,
    _input: midi_connection::ThreadReference,
    offset: u8,
    instance: usize,
    output_values: HashMap<u32, (u8, u8, u8)>,
    velocity_map: Option<Vec<u8>>,
    offset_press: Arc<Mutex<OffsetPress>>,
    sidechain_output: Option<SidechainOutput>,
    updating: bool,
}

impl Sp404Mk2 {
    pub fn new(
        port_name: &str,
        instance: usize,
        default_offset: u8,
        velocity_map: Option<Vec<u8>>,
        sidechain_output: Option<SidechainOutput>,
    ) -> Self {
        let output = midi_connection::get_shared_output(port_name);
        let offset_press = Arc::new(Mutex::new(OffsetPress::None));
        let input_offset_press = offset_press.clone();

        let input = midi_connection::get_input(port_name, move |_stamp, message| {
            if message[0] >= 144 && message[0] < 154 {
                let bank = message[0] - 144;
                let pad = message[1] - 36;
                let sub_bank = pad / 8;
                let value = bank * 2 + sub_bank;

                if pad == 3 || pad == 11 {
                    let mut offset_press = input_offset_press.lock().unwrap();

                    let count = offset_press.value_count(value) + 1;

                    *offset_press = OffsetPress::Pressed {
                        instant: Instant::now(),
                        value,
                        count,
                    };
                }
            }
        });

        Sp404Mk2 {
            output,
            instance,
            output_values: HashMap::new(),
            offset: default_offset,
            offset_press,
            velocity_map,
            sidechain_output,
            _input: input,
            updating: false,
        }
    }
}

impl Triggerable for Sp404Mk2 {
    fn on_tick(&mut self, _time: MidiTime) {
        self.updating = false;
        let mut offset_press = self.offset_press.lock().unwrap();

        // update the offset if the number of presses match the instance ID after 200ms
        if let Some((value, count)) =
            offset_press.get_before(Instant::now() - Duration::from_millis(200))
        {
            if self.instance == count - 1 {
                self.offset = value;
                // make the lights flash off for a tick
                self.updating = true
            }

            *offset_press = OffsetPress::None;
        }
    }

    fn check_lit(&self, id: u32) -> bool {
        !self.updating
    }

    fn trigger(&mut self, id: u32, value: OutputValue) {
        match value {
            OutputValue::Off => {
                if self.output_values.contains_key(&id) {
                    let (channel_offset, note, _velocity) =
                        self.output_values.get(&id).unwrap().clone();

                    self.output.send(&[128 + channel_offset, note, 0]).unwrap();
                    self.output_values.remove(&id);
                }
            }
            OutputValue::On(velocity) => {
                let velocity = ::devices::map_velocity(&self.velocity_map, velocity);
                let channel_offset = self.offset / 2;
                let sub_bank = self.offset % 2;
                let note = SUB_NOTES[id as usize % SUB_NOTES.len()] + 36 + (8 * sub_bank);

                self.output
                    .send(&[144 + channel_offset, note, velocity])
                    .unwrap();

                // send sync if kick
                if let Some(sidechain_output) = &mut self.sidechain_output {
                    if id == sidechain_output.id {
                        let mut params = sidechain_output.params.lock().unwrap();
                        params.duck_triggered = true;
                    }
                }

                self.output_values
                    .insert(id, (channel_offset, note, velocity));
            }
        }
    }
}
#[derive(Serialize, Deserialize, PartialEq, Clone)]
pub enum MidiTrigger {
    Cc(u8, u8, u8),
    CcVelocity(u8, u8),
    Note(u8, u8, u8),
    NoteVelocity(u8, u8),
    Multi(Vec<MidiTrigger>),
    None,
}
