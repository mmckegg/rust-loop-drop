use ::clock_source::{MidiTime};

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
    pub skew: u8, 
    pub hold: u8,
    pub speed: u8,
    pub offset: u8
}

impl Lfo {
    // Returns a value between 0 and 1
    pub fn new () -> Self {
        Lfo {
            skew: 0,
            hold: 0,
            speed: 50,
            offset: 64
        }
    }
    pub fn get_value_at (&self, pos: MidiTime) -> f64 { 
        let rate_index = (self.speed as f64 * (RATES.len() as f64 / 128.0)) as usize;
        let cycle_duration = RATES[rate_index];
        let offset = MidiTime::from_float(cycle_duration.as_float() * ((self.offset as f64 - 64.0) / 64.0) / 2.0);
        let phase = ((pos + offset) % cycle_duration).as_float() / cycle_duration.as_float();
        let mid = self.skew as f64 / 127.0;
        let hold = self.hold as f64 / 127.0;
        if mid <= 0.0 {
            1.0 - get_held_pos(phase, hold)
        } else if phase < mid {
            get_held_pos(phase / mid, hold)
        } else {
            1.0 - get_held_pos((phase - mid) / (1.0 - mid), hold)
        }
    }
}

fn get_held_pos (pos: f64, hold: f64) -> f64 {
    if pos < (1.0 - hold) {
        pos / (1.0 - hold)
    } else {
        1.0
    }
}