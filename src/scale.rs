use std::sync::{Arc, Mutex};
use std::collections::HashSet;

#[derive(Clone, Eq, PartialEq)]
pub struct Scale {
    pub root: i32,
    pub scale: i32,
    pub offset: i32
}

impl Scale {

    pub fn new (root: i32, scale: i32) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Scale { 
            root, 
            scale,
            offset: 0
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
        let intervals = [2, 2, 1, 2, 2, 2, 1];
        let mut scale_notes = vec![0];
        let mut last_value = 0;
        for i in 0..6 {
            last_value += intervals[modulo(i + self.scale, 7) as usize];
            scale_notes.push(last_value);
        }
        let length = scale_notes.len() as i32;
        let interval = scale_notes[modulo(value, length) as usize];
        let octave = (value as f64 / length as f64).floor() as i32;
        self.root + self.offset + (octave * 12) + interval
    }
}

fn modulo (n: i32, m: i32) -> i32 {
    ((n % m) + m) % m
}

#[derive(Clone, Eq, PartialEq)]
pub struct Offset {
    pub base: i32,
    pub offset: i32,
    pub pitch: i32
}

impl Offset {
    pub fn new (oct: i32, base: i32) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Offset {
            offset: 0,
            base,
            pitch: 0
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_major() {
        let scale_arc = Scale::new(0, 0);
        let scale = scale_arc.lock().unwrap();
        let result: Vec<i32> = (-2..9).map(|i| scale.get_note_at(i)).collect();
        assert_eq!(result, vec![
            -3, -1, 0, 2, 4, 5, 7, 9, 11, 12, 14
        ]);
    }

    #[test]
    fn check_natural_minor() {
        let scale_arc = Scale::new(0, 5);
        let scale = scale_arc.lock().unwrap();
        let result: Vec<i32> = (-2..9).map(|i| scale.get_note_at(i)).collect();
        assert_eq!(result, vec![
            -4, -2, 0, 2, 3, 5, 7, 8, 10, 12, 14
        ]);
    }
}