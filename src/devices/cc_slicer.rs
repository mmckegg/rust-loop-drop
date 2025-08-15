use chunk::{MidiTime, OutputValue, Triggerable};
use midi_connection;

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use crate::{
    controllers::{midi_to_polar, polar_to_msb_lsb},
    loop_grid_launchpad::LoopGridParams,
};

pub struct CcSlicer {
    params: Arc<Mutex<LoopGridParams>>,
    midi_port: midi_connection::SharedMidiOutputConnection,
    midi_channel: u8,
    last_pos: MidiTime,
    velocity_map: Option<Vec<u8>>,
    active_ids: HashSet<u32>,
    slicer_channel: u32,
    cc: u8,
}

impl CcSlicer {
    pub fn new(
        params: Arc<Mutex<LoopGridParams>>,
        midi_port: midi_connection::SharedMidiOutputConnection,
        channel: u8,
        slicer_channel: u32,
        cc: u8,
        velocity_map: Option<Vec<u8>>,
    ) -> Self {
        CcSlicer {
            params,
            midi_port,
            slicer_channel,
            cc,
            last_pos: MidiTime::zero(),
            midi_channel: channel,
            active_ids: HashSet::new(),
            velocity_map,
        }
    }
}

impl Triggerable for CcSlicer {
    fn on_tick(&mut self, time: MidiTime) {
        self.last_pos = time;
    }

    fn trigger(&mut self, id: u32, value: OutputValue) {
        let channel = self.midi_channel;
        let note_id = 60;

        match value {
            OutputValue::Off => {
                // Only act if this id was active
                if self.active_ids.remove(&id) {
                    // If no other triggers are active, send the actual Note Off
                    if self.active_ids.is_empty() {
                        let _ = self.midi_port.send(&[144 - 1 + channel, note_id, 0]);
                    }
                }
            }
            OutputValue::On(velocity) => {
                let params = self.params.lock().unwrap();

                let midi_pitch = params
                    .slicer_pitches
                    .get(&self.slicer_channel)
                    .and_then(|map| map.get(&id))
                    .copied()
                    .unwrap_or(0);

                let cc_val = params
                    .slicer_offsets
                    .get(&self.slicer_channel)
                    .and_then(|map| map.get(&id))
                    .copied()
                    .unwrap_or(0);

                let mapped_velocity = ::devices::map_velocity(&self.velocity_map, velocity);

                let f_pitch = midi_to_polar(midi_pitch);
                let (msb, lsb) = polar_to_msb_lsb(f_pitch);

                // Update CC and pitch before the note retrigger
                let _ = self.midi_port.send(&[176 - 1 + channel, self.cc, cc_val]);
                let _ = self.midi_port.send(&[224 - 1 + channel, msb, lsb]);

                // If something else is already active, choke (Note Off) then retrigger
                if !self.active_ids.is_empty() {
                    let _ = self.midi_port.send(&[144 - 1 + channel, note_id, 0]);
                }

                // Send Note On (retrigger or initial)
                let _ = self
                    .midi_port
                    .send(&[144 - 1 + channel, note_id, mapped_velocity]);

                // Mark this id as active
                self.active_ids.insert(id);
            }
        }
    }
}
