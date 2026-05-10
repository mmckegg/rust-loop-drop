use chunk::{MidiTime, OutputValue, Triggerable};
use midi_connection;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::{
    controllers::{midi_to_float, midi_to_polar, polar_to_msb_lsb},
    loop_grid::LoopGridParams,
};

pub struct MidiSlicer {
    params: Arc<Mutex<LoopGridParams>>,
    midi_port: midi_connection::SharedMidiOutputConnection,
    midi_channel: u8,
    last_pos: MidiTime,
    velocity_map: Option<Vec<u8>>,
    output_values: HashMap<u32, (u8, u8, u8)>,
    slicer_channel: u32,
    trigger_count: u32,
    start_trigger_id: u8,
}

impl MidiSlicer {
    pub fn new(
        params: Arc<Mutex<LoopGridParams>>,
        midi_port: midi_connection::SharedMidiOutputConnection,
        channel: u8,
        slicer_channel: u32,
        start_trigger_id: u8,
        trigger_count: u32,
        velocity_map: Option<Vec<u8>>,
    ) -> Self {
        MidiSlicer {
            params,
            midi_port,
            slicer_channel,
            start_trigger_id,
            trigger_count,
            last_pos: MidiTime::zero(),
            midi_channel: channel,
            output_values: HashMap::new(),
            velocity_map,
        }
    }
}

impl Triggerable for MidiSlicer {
    fn on_tick(&mut self, time: MidiTime) {
        self.last_pos = time;
    }

    fn trigger(&mut self, id: u32, value: OutputValue) {
        match value {
            OutputValue::Off => {
                if self.output_values.contains_key(&id) {
                    let (channel, note_id, _) = *self.output_values.get(&id).unwrap();
                    self.midi_port
                        .send(&[144 - 1 + channel, note_id, 0])
                        .unwrap();
                    self.output_values.remove(&id);
                }
            }
            OutputValue::On(velocity) => {
                let params = self.params.lock().unwrap();

                let midi_pitch = params
                    .slicer_pitches
                    .get(&self.slicer_channel)
                    .and_then(|map| map.get(&id))
                    .unwrap_or(&0);

                let start = params
                    .slicer_offsets
                    .get(&self.slicer_channel)
                    .and_then(|map| map.get(&0))
                    .unwrap_or(&0);

                let end = params
                    .slicer_offsets
                    .get(&self.slicer_channel)
                    .and_then(|map| map.get(&(self.trigger_count - 1)))
                    .unwrap_or(&0);

                let all_together = end <= start;

                let value = if all_together {
                    start.saturating_add(id as u8)
                } else if id == 0 {
                    *start
                } else if id == self.trigger_count - 1 {
                    *end
                } else {
                    let range = end - start;
                    let midi_val = params
                        .slicer_offsets
                        .get(&self.slicer_channel)
                        .and_then(|map| map.get(&id))
                        .unwrap_or(&0);

                    let f_val = midi_to_float(*midi_val);
                    start + (range as f64 * f_val) as u8
                };

                let channel = self.midi_channel;
                let note_id = self.start_trigger_id.saturating_add(value).min(127);
                let mapped_velocity = crate::devices::map_velocity(&self.velocity_map, velocity);

                let f_pitch = midi_to_polar(*midi_pitch);
                let (msb, lsb) = polar_to_msb_lsb(f_pitch);

                // send note
                self.midi_port
                    .send(&[144 - 1 + channel, note_id, mapped_velocity])
                    .unwrap();

                // send pitch
                self.midi_port.send(&[224 - 1 + channel, msb, lsb]).unwrap();

                self.output_values
                    .insert(id, (channel, note_id, mapped_velocity));
            }
        }
    }
}
