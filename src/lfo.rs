use ::MidiTime;

lazy_static! {
    static ref RATES: [MidiTime; 10] = [
        MidiTime::from_measure(3, 1),
        MidiTime::from_measure(2, 1),
        MidiTime::from_measure(3, 2),
        MidiTime::from_measure(1, 1),
        MidiTime::from_measure(2, 3),
        MidiTime::from_measure(1, 2),
        MidiTime::from_measure(1, 3),
        MidiTime::from_measure(1, 4),
        MidiTime::from_measure(1, 6),
        MidiTime::from_measure(1, 8)
    ];
}

// midi 0-127 for all values
pub struct Lfo {
    pub speed: u8,
    pub wave: u8,
}

impl Lfo {
    // Returns a value between 0 and 1
    pub fn new() -> Self {
        Lfo { speed: 50, wave: 64 }
    }

    pub fn get_value_at(&self, pos: MidiTime) -> f64 {
        let rate_index = (self.speed as f64 * (RATES.len() as f64 / 128.0)) as usize;
        let cycle_duration = RATES[rate_index.min(RATES.len() - 1)];
        let phase = (pos % cycle_duration).as_float() / cycle_duration.as_float();
        let wave = self.wave as f64 / 127.0;

        if wave < 0.2 {
            triangle(phase)
        } else if wave < 0.4 {
            phase
        } else if wave < 0.6 {
            held_ramp(phase, remap(wave, 0.4, 0.6, 0.0, 0.8))
        } else if wave < 0.8 {
            1.0 - held_ramp(phase, remap(wave, 0.6, 0.8, 0.0, 0.8))
        } else {
            1.0 - phase
        }
    }
}

fn triangle(phase: f64) -> f64 {
    if phase < 0.5 {
        phase * 2.0
    } else {
        1.0 - ((phase - 0.5) * 2.0)
    }
}

fn held_ramp(phase: f64, hold: f64) -> f64 {
    if phase < (1.0 - hold) {
        phase / (1.0 - hold)
    } else {
        1.0
    }
}

fn remap(value: f64, in_min: f64, in_max: f64, out_min: f64, out_max: f64) -> f64 {
    let normalized = ((value - in_min) / (in_max - in_min)).max(0.0).min(1.0);
    out_min + normalized * (out_max - out_min)
}
