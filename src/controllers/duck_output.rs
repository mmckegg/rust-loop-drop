use std::sync::{Arc, Mutex};

use midi_time::MidiTime;

use crate::{loop_grid_launchpad::LoopGridParams, trigger_envelope::TriggerEnvelope};

use super::float_to_midi;

pub struct DuckOutput {
    modulators: Vec<::controllers::Modulator>,
    trigger_envelope: TriggerEnvelope,
    params: Arc<Mutex<LoopGridParams>>,
}

impl DuckOutput {
    pub fn new(
        modulators: Vec<::controllers::Modulator>,
        params: Arc<Mutex<LoopGridParams>>,
    ) -> Self {
        let duck_tick_multiplier = params.lock().unwrap().duck_tick_multiplier;
        let trigger_envelope = TriggerEnvelope::new(duck_tick_multiplier, 0.5);
        DuckOutput {
            modulators,
            params,
            trigger_envelope,
        }
    }
}

impl ::controllers::Schedulable for DuckOutput {
    fn schedule(&mut self, _pos: MidiTime, _length: MidiTime) {
        {
            let params = self.params.lock().unwrap();
            self.trigger_envelope.tick_multiplier = params.duck_tick_multiplier;
            self.trigger_envelope.tick(params.duck_triggered);
        }

        for modulator in &mut self.modulators {
            if let ::controllers::Modulator::MidiModulator(instance) = modulator {
                let f_value = self.trigger_envelope.value().powf(0.5);
                let value = float_to_midi(f_value);
                instance.send(value)
            }
        }
    }
}
