use indexmap::IndexSet;

use chunk::{LatchMode, OutputValue, ScheduleMode, Triggerable};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

pub use scale::{Offset, Scale};

use crate::config::{PerfectQuality, Quality, ScaleDegree};
use crate::loop_grid_launchpad::LoopGridParams;

pub struct ScaleDegreeToggle {
    scale: Arc<Mutex<Scale>>,
    degree: ScaleDegree,
    stack: IndexSet<u32>,
    params: Arc<Mutex<LoopGridParams>>,
}

impl ScaleDegreeToggle {
    pub fn new(
        scale: Arc<Mutex<Scale>>,
        degree: ScaleDegree,
        params: Arc<Mutex<LoopGridParams>>,
    ) -> Self {
        ScaleDegreeToggle {
            scale,
            degree,
            stack: IndexSet::new(),
            params,
        }
    }

    fn refresh_output(&mut self) {
        if let Some(id) = self.stack.last().cloned() {
            let mut current_scale = self.scale.lock().unwrap();

            match self.degree {
                ScaleDegree::Second => current_scale.second = id_to_quality(id),
                ScaleDegree::Third => current_scale.third = id_to_quality(id),
                ScaleDegree::Fourth => current_scale.fourth = id_to_augmented_quality(id),
                ScaleDegree::Fifth => current_scale.fifth = id_to_diminished_quality(id),
                ScaleDegree::Sixth => current_scale.sixth = id_to_quality(id),
                ScaleDegree::Seventh => current_scale.seventh = id_to_quality(id),
            }
        }
    }
}

impl Triggerable for ScaleDegreeToggle {
    fn trigger(&mut self, id: u32, value: OutputValue) {
        match value {
            OutputValue::Off => {
                self.stack.shift_remove(&id);
                self.refresh_output();
            }
            OutputValue::On(_velocity) => {
                self.stack.insert(id);
                self.refresh_output();
            }
        }
    }

    fn check_triggering(&self, id: u32) -> Option<bool> {
        let current_scale = self.scale.lock().unwrap();

        let second_active = match self.degree {
            ScaleDegree::Second => current_scale.second == Quality::Minor,
            ScaleDegree::Third => current_scale.third == Quality::Minor,
            ScaleDegree::Fourth => current_scale.fourth == PerfectQuality::Augmented,
            ScaleDegree::Fifth => current_scale.fifth == PerfectQuality::Diminished,
            ScaleDegree::Sixth => current_scale.sixth == Quality::Minor,
            ScaleDegree::Seventh => current_scale.seventh == Quality::Minor,
        };

        if id == 0 && !second_active || id == 1 && second_active {
            Some(true)
        } else {
            Some(false)
        }
    }

    fn check_lit(&self, _: u32) -> bool {
        let params = self.params.lock().unwrap();
        let scale = self.scale.lock().unwrap();
        params
            .active_notes
            .iter()
            .any(|note| scale.get_degree_of_note(*note as i32) == Some(self.degree.clone()))
    }

    fn schedule_mode(&self) -> ScheduleMode {
        ScheduleMode::Monophonic
    }
}

fn id_to_quality(id: u32) -> Quality {
    if id == 0 {
        Quality::Major
    } else {
        Quality::Minor
    }
}

fn id_to_augmented_quality(id: u32) -> PerfectQuality {
    if id == 0 {
        PerfectQuality::Perfect
    } else {
        PerfectQuality::Augmented
    }
}
fn id_to_diminished_quality(id: u32) -> PerfectQuality {
    if id == 0 {
        PerfectQuality::Perfect
    } else {
        PerfectQuality::Diminished
    }
}
