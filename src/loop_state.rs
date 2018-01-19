use std::collections::HashMap;
use ::midi_time::MidiTime;

#[derive(Debug, Clone)]
pub struct Loop {
    pub length: MidiTime,
    pub offset: MidiTime,
    pub transforms: HashMap<u32, LoopTransform>
}

impl Loop {
    pub fn new (offset: MidiTime, length: MidiTime) -> Loop {
        Loop {
            offset,
            length,
            transforms: HashMap::new()
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum LoopTransform {
    On,
    None,
    Repeat(MidiTime, MidiTime),
    Hold(MidiTime, MidiTime),
    Suppress
}

pub struct LoopState {
    undos: Vec<Loop>,
    redos: Vec<Loop>,
    on_change: Box<FnMut(&Loop) + Send>
}

impl LoopState {
    pub fn new<F> (default_length: MidiTime, on_change: F) -> LoopState
    where F: FnMut(&Loop) + Send + 'static  {
        let default_loop = Loop::new(MidiTime::zero() - default_length, default_length);
        LoopState {
            undos: vec![default_loop],
            redos: Vec::new(),
            on_change: Box::new(on_change)
        }
    }

    pub fn get (&self) -> &Loop {
        &self.undos.last().unwrap()
    }

    pub fn set (&mut self, value: Loop) {
        self.undos.push(value);
        (self.on_change)(self.undos.last().unwrap());
    }

    pub fn undo (&mut self) {
        if self.undos.len() > 1 {
            match self.undos.pop() {
                Some(value) => {
                    self.redos.push(value);
                    (self.on_change)(self.undos.last().unwrap());
                },
                None => ()
            };
        }
    }

    pub fn redo (&mut self) {
        match self.redos.pop() {
            Some(value) => {
                self.undos.push(value);
                (self.on_change)(self.undos.last().unwrap());
            },
            None => ()
        };
    }
}