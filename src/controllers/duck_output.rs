use std::sync::{Arc, Mutex};
use crate::{
    loop_grid_launchpad::LoopGridParams, scheduler::ScheduleRange,
    trigger_envelope::TriggerEnvelope,
};

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
        let mut trigger_envelope = TriggerEnvelope::new(duck_tick_multiplier, 2.0);
        trigger_envelope.tick_value = 2.0;
        DuckOutput {
            modulators,
            params,
            trigger_envelope,
        }
    }
}

impl ::controllers::Schedulable for DuckOutput {
    fn schedule(&mut self, range: ScheduleRange) {
        if range.ticked {
            let reduction_amount;
            {
                let params = self.params.lock().unwrap();
                self.trigger_envelope.tick_multiplier = params.duck_tick_multiplier;
                self.trigger_envelope.tick(params.duck_triggered);
                reduction_amount = params.duck_reduction.clone();
            }

            for modulator in &mut self.modulators {
                if let ::controllers::Modulator::MidiModulator(instance) = modulator {
                    let f_value = self.trigger_envelope.value().powf(0.5);
                    let value = float_to_midi(f_value * reduction_amount);
                    instance.send(value)
                }
            }
        }
    }
}
