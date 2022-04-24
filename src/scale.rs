use std::sync::{Arc, Mutex};

use crate::config::{PerfectQuality, Quality, ScaleDegree};

const PENTATONIC: [i32; 5] = [0, 2, 5, 7, 9];
const MINOR_PENTATONIC: [i32; 5] = [0, 3, 5, 7, 10];
const LENGTH: usize = 7;

#[derive(Clone, Eq, PartialEq)]
pub struct Scale {
    pub root: i32,
    pub offset: i32,
    pub second: Quality,
    pub third: Quality,
    pub fourth: PerfectQuality,
    pub fifth: PerfectQuality,
    pub sixth: Quality,
    pub seventh: Quality,
}

impl Scale {
    pub fn new(root: i32) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Scale {
            root,
            offset: 0,
            second: Quality::Major,
            third: Quality::Major,
            fourth: PerfectQuality::Perfect,
            fifth: PerfectQuality::Perfect,
            sixth: Quality::Major,
            seventh: Quality::Major,
        }))
    }

    pub fn get_pentatonic_at(&self, index: i32) -> bool {
        if self.third == Quality::Minor && self.seventh == Quality::Minor {
            MINOR_PENTATONIC.contains(&self.get_interval_at(index))
        } else {
            PENTATONIC.contains(&self.get_interval_at(index))
        }
    }

    pub fn get_interval_at(&self, index: i32) -> i32 {
        let scale_index = modulo(index, LENGTH as i32);

        match scale_index {
            0 => 0, // root
            1 => 2 + self.second as i32,
            2 => 4 + self.third as i32,
            3 => 5 + self.fourth as i32,
            4 => 7 + self.fifth as i32,
            5 => 9 + self.sixth as i32,
            6 => 11 + self.seventh as i32,
            _ => 0,
        }
    }

    pub fn get_degree_of_note(&self, note: i32) -> Option<ScaleDegree> {
        let note = modulo(note - self.root, 12);

        if note == self.get_interval_at(1) {
            Some(ScaleDegree::Second)
        } else if note == self.get_interval_at(2) {
            Some(ScaleDegree::Third)
        } else if note == self.get_interval_at(3) {
            Some(ScaleDegree::Fourth)
        } else if note == self.get_interval_at(4) {
            Some(ScaleDegree::Fifth)
        } else if note == self.get_interval_at(5) {
            Some(ScaleDegree::Sixth)
        } else if note == self.get_interval_at(6) {
            Some(ScaleDegree::Seventh)
        } else {
            None
        }
    }

    pub fn get_note_at(&self, index: i32) -> i32 {
        let octave = (index as f64 / LENGTH as f64).floor() as i32;
        self.root + self.offset + (octave * 12) + self.get_interval_at(index)
    }
}

fn modulo(n: i32, m: i32) -> i32 {
    ((n % m) + m) % m
}

#[derive(Clone, Eq, PartialEq)]
pub struct Offset {
    pub base: i32,
    pub offset: i32,
    pub pitch: i32,
}

impl Offset {
    pub fn new(base: i32) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Offset {
            offset: 0,
            base,
            pitch: 0,
        }))
    }
}
