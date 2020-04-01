use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::mpsc;

use ::midi_time::MidiTime;
pub use ::loop_transform::LoopTransform;

#[derive(Debug, Clone)]
pub struct LoopCollection {
    pub length: MidiTime,
    pub transforms: HashMap<u32, LoopTransform>
}

#[derive(Eq, PartialEq)]
pub enum LoopStateChange {
    Undo, Redo, Set
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
    pub change_queue: mpsc::Receiver<LoopStateChange>,
    change_queue_tx: mpsc::Sender<LoopStateChange>,

    undos: Vec<LoopCollection>,
    redos: Vec<LoopCollection>
}

impl LoopState {
    pub fn new (default_length: MidiTime) -> LoopState {
        let default_loop = LoopCollection::new(default_length);
        let (change_queue_tx, change_queue) = mpsc::channel();
        LoopState {
            undos: vec![default_loop],
            redos: Vec::new(),
            change_queue_tx,
            change_queue
        }
    }

    pub fn get (&self) -> &LoopCollection {
        &self.undos.last().unwrap()
    }

    pub fn retrieve (&self, offset: isize) -> Option<&LoopCollection> {
        if offset < 0 {
            let resolved_offset = self.undos.len() as isize - 1 + offset;
            if resolved_offset > 0 {
                self.undos.get(resolved_offset as usize)
            } else {
                None
            }
        } else if offset > 0 {
            let resolved_offset = self.redos.len() as isize - 1 - offset;
            if resolved_offset > 0 {
                self.redos.get(resolved_offset as usize)
            } else {
                None
            }
        } else {
            Some(&self.get())
        }
    }

    pub fn next_index_for (&self, current_offset: isize, selection: &HashSet<u32>) -> Option<isize> {
        self.index_from(current_offset, 1, selection)
    }

    pub fn previous_index_for (&self, current_offset: isize, selection: &HashSet<u32>) -> Option<isize> {
        self.index_from(current_offset, -1, selection)
    }

    pub fn set (&mut self, value: LoopCollection) {
        self.undos.push(value);
        self.on_change(LoopStateChange::Set);
    }

    pub fn undo (&mut self) {
        if self.undos.len() > 1 {
            match self.undos.pop() {
                Some(value) => {
                    self.redos.push(value);
                    self.on_change(LoopStateChange::Undo);
                },
                None => ()
            };
        }
    }

    pub fn redo (&mut self) {
        match self.redos.pop() {
            Some(value) => {
                self.undos.push(value);
                self.on_change(LoopStateChange::Redo);
            },
            None => ()
        };
    }

    fn index_from (&self, current_offset: isize, request_offset: isize, selection: &HashSet<u32>) -> Option<isize> {
        if let Some(start_item) = self.retrieve(current_offset) {
            let mut item = Some(start_item);
            let mut offset = current_offset;

            // keep going until we run out or the transforms are different for given range
            while item.is_some() {
                offset = offset + request_offset;
                item = self.retrieve(offset);    

                if let Some(item) = item {
                    if selection.iter().any(|id| start_item.transforms.get(id) != item.transforms.get(id)) {
                        return Some(offset)
                    }
                }
            } 
        }

        None
    }

    fn on_change (&self, change: LoopStateChange) {
        self.change_queue_tx.send(change).unwrap();
    }
}