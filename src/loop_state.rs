use std::collections::HashMap;
use ::midi_time::MidiTime;
pub use ::loop_transform::LoopTransform;

#[derive(Debug, Clone)]
pub struct LoopCollection {
    pub length: MidiTime,
    pub transforms: HashMap<u32, LoopTransform>
}

impl LoopCollection {
    pub fn new (length: MidiTime) -> LoopCollection {
        LoopCollection {
            length,
            transforms: HashMap::new()
        }
    }
}

pub struct LoopState {
    undos: Vec<LoopCollection>,
    redos: Vec<LoopCollection>,
    on_change: Box<FnMut(&LoopCollection) + Send>
}

impl LoopState {
    pub fn new<F> (default_length: MidiTime, on_change: F) -> LoopState
    where F: FnMut(&LoopCollection) + Send + 'static  {
        let default_loop = LoopCollection::new(default_length);
        LoopState {
            undos: vec![default_loop],
            redos: Vec::new(),
            on_change: Box::new(on_change)
        }
    }

    pub fn get (&self) -> &LoopCollection {
        &self.undos.last().unwrap()
    }

    pub fn set (&mut self, value: LoopCollection) {
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