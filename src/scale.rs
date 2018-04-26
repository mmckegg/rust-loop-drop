use std::sync::{Arc, Mutex};
use std::collections::HashSet;

#[derive(Clone, Eq, PartialEq)]
pub struct Scale {
    pub root: i32,
    pub scale: i32,
    pub sample_group_a: u32,
    pub sample_group_b: u32
}

impl Scale {

    pub fn new (root: i32, scale: i32) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Scale { 
            root, 
            scale,
            sample_group_a: 0,
            sample_group_b: 0
        }))
    }

    pub fn get_notes (&self) -> HashSet<i32> {
        let mut result = HashSet::new();
        for i in -100..100 {
            result.insert(self.get_note_at(i));
        }
        result
    }

    pub fn get_note_at (&self, value: i32) -> i32 {
        let intervals = [2, 2, 1, 2, 2, 1];
        let mut scale_notes = vec![0];
        let mut last_value = 0;
        for i in 0..6 {
            last_value += intervals[modulo(i + self.scale, 6) as usize];
            scale_notes.push(last_value);
        }
        let length = scale_notes.len() as i32;
        let interval = scale_notes[modulo(value, length) as usize];
        let octave = (value as f64 / length as f64).floor() as i32;
        self.root + (octave * 12) + interval
    }
}

fn modulo (n: i32, m: i32) -> i32 {
    ((n % m) + m) % m
}

#[derive(Clone, Eq, PartialEq)]
pub struct Offset {
    pub oct: i32,
    pub third: i32,
    pub offset: i32,
    pub pitch: i32
}

impl Offset {
    pub fn new (oct: i32) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Offset {
            oct, 
            third: 0,
            offset: 0,
            pitch: 0
        }))
    }
}