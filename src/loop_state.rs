use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::mpsc;

pub use loop_transform::LoopTransform;
use midi_time::MidiTime;

#[derive(Debug, Clone, PartialEq)]
pub struct LoopCollection {
    pub length: MidiTime,
    pub transforms: HashMap<u32, LoopTransform>,
}

#[derive(Eq, PartialEq)]
pub enum LoopStateChange {
    Undo,
    Redo,
    Set,
}

impl LoopCollection {
    pub fn new(length: MidiTime) -> LoopCollection {
        LoopCollection {
            length,
            transforms: HashMap::new(),
        }
    }
}

pub struct LoopState {
    pub change_queue: mpsc::Receiver<LoopStateChange>,
    change_queue_tx: mpsc::Sender<LoopStateChange>,

    frozen: bool,
    override_loop: Option<LoopCollection>,
    undos: Vec<LoopCollection>,
    redos: Vec<LoopCollection>,
}

impl LoopState {
    pub fn new(default_length: MidiTime) -> LoopState {
        let default_loop = LoopCollection::new(default_length);
        let (change_queue_tx, change_queue) = mpsc::channel();
        LoopState {
            override_loop: None,
            frozen: false,
            undos: vec![default_loop],
            redos: Vec::new(),
            change_queue_tx,
            change_queue,
        }
    }

    pub fn get(&self) -> &LoopCollection {
        if let Some(ref override_loop) = self.override_loop {
            &override_loop
        } else {
            &self.undos.last().unwrap()
        }
    }

    pub fn retrieve(&self, offset: isize) -> Option<&LoopCollection> {
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

    pub fn next_index_for(&self, current_offset: isize, selection: &HashSet<u32>) -> Option<isize> {
        self.index_from(current_offset, 1, selection)
    }

    pub fn previous_index_for(
        &self,
        current_offset: isize,
        selection: &HashSet<u32>,
    ) -> Option<isize> {
        self.index_from(current_offset, -1, selection)
    }

    pub fn set(&mut self, value: LoopCollection) {
        if self.get() == &value {
            // ignore setting the same value multiple times
            return;
        }
        if self.frozen {
            self.override_loop = Some(value);
        } else {
            self.undos.push(value);
        }
        self.on_change(LoopStateChange::Set);
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn unfreeze(&mut self) {
        self.frozen = false;
        let had_loop = self.override_loop.is_some();
        self.override_loop = None;
        if had_loop {
            self.on_change(LoopStateChange::Set);
        }
    }

    pub fn undo(&mut self) {
        if self.override_loop.is_some() {
            self.override_loop = None;
            self.on_change(LoopStateChange::Undo);
        } else if self.undos.len() > 1 {
            match self.undos.pop() {
                Some(value) => {
                    self.redos.push(value);
                    self.on_change(LoopStateChange::Undo);
                }
                None => (),
            };
        }
    }

    pub fn redo(&mut self) {
        if self.override_loop.is_some() {
            self.override_loop = None;
            self.on_change(LoopStateChange::Redo);
        } else {
            match self.redos.pop() {
                Some(value) => {
                    self.undos.push(value);
                    self.on_change(LoopStateChange::Redo);
                }
                None => (),
            };
        }
    }

    fn index_from(
        &self,
        current_offset: isize,
        request_offset: isize,
        selection: &HashSet<u32>,
    ) -> Option<isize> {
        if let Some(start_item) = self.retrieve(current_offset) {
            let mut item = Some(start_item);
            let mut offset = current_offset;

            // keep going until we run out or the transforms are different for given range
            while item.is_some() {
                if let Some(item) = item {
                    if selection
                        .iter()
                        .any(|id| start_item.transforms.get(id) != item.transforms.get(id))
                    {
                        return Some(offset);
                    }
                }

                offset = offset + request_offset;
                item = self.retrieve(offset);
            }
        }

        None
    }

    fn on_change(&self, change: LoopStateChange) {
        self.change_queue_tx.send(change).unwrap();
    }
}
