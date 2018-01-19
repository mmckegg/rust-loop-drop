use std::cmp::Ordering;
use std::collections::{HashSet, HashMap};
use ::midi_time::MidiTime;

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum OutputValue {
    On, Off
}

#[derive(Debug, Copy, Clone)]
pub struct LoopEvent {
    pub value: OutputValue,
    pub pos: MidiTime,
    pub id: u32
}

impl LoopEvent {
    pub fn with_pos (&self, new_pos: MidiTime) -> LoopEvent {
        LoopEvent {
            id: self.id,
            value: self.value.clone(),
            pos: new_pos
        }
    }
}

impl PartialOrd for LoopEvent {
    fn partial_cmp(&self, other: &LoopEvent) -> Option<Ordering> {
        // Some(self.cmp(other))
        let value = self.pos.partial_cmp(&other.pos).unwrap();
        if self.eq(other) {
            // replace the item if same type, 
            Some(Ordering::Equal)
        } else if value == Ordering::Equal {
            // or insert after if different (but same position)
            Some(self.id.cmp(&other.id))
        } else {
            Some(value)
        }
    }
}

impl PartialEq for LoopEvent {
    fn eq(&self, other: &LoopEvent) -> bool {
        self.pos == other.pos && self.value == other.value && self.id == other.id
    }
}

pub struct LoopRecorder {
    history: Vec<LoopEvent>,
    per_id: HashMap<u32, Vec<LoopEvent>>
}

impl LoopRecorder {
    pub fn new () -> Self {
        Self {
            history: Vec::new(),
            per_id: HashMap::new()
        }
    }

    pub fn add (&mut self, event: LoopEvent) {

        // record events per slot
        let collection = self.per_id.entry(event.id).or_insert(Vec::new());
        match collection.binary_search_by(|v| {
            v.partial_cmp(&event).expect("Cannot compare events (NaN?)")
        }) {
            Ok(index) => {
                collection.push(event);
                // swap_remove removes at index and puts last item in its place
                collection.swap_remove(index); 
            },
            Err(index) => collection.insert(index, event)
        };

        // also record all mixed together (for easy looping)
        match self.history.binary_search_by(|v| {
            v.partial_cmp(&event).expect("Cannot compare events (NaN?)")
        }) {
            Ok(index) => {
                self.history.push(event);
                // swap_remove removes at index and puts last item in its place
                self.history.swap_remove(index); 
            },
            Err(index) => self.history.insert(index, event)
        };
    }

    pub fn get_ids_in_range (&self, start_pos: MidiTime, end_pos: MidiTime) -> HashSet<u32> {
        let mut result: HashSet<u32> = HashSet::new();

        for event in self.get_range(start_pos, end_pos) {
            if event.value != OutputValue::Off {
                result.insert(event.id);
            }
        }

        result 
    }

    pub fn get_range (&self, start_pos: MidiTime, end_pos: MidiTime) -> &[LoopEvent] {
        let start_index = match self.history.binary_search_by(|v| {
            if v.pos < start_pos {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }) {
            Ok(index) | Err(index) => index,
        };

        let end_index = match self.history.binary_search_by(|v| {
            if v.pos < end_pos {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }) {
            Ok(index) | Err(index) => index,
        };

        &self.history[start_index..end_index]
    }

    pub fn get_event_at (&self, id: u32, pos: MidiTime) -> Option<&LoopEvent> {
        if let Some(collection) = self.per_id.get(&id) {
            match collection.binary_search_by(|v| {
                v.pos.partial_cmp(&pos).unwrap()
            }) {
                Ok(index) => collection.get(index),
                Err(index) => if index > 0 {
                    collection.get(index - 1)
                } else {
                    None
                }
            }
        } else {
            None
        }
    }

    pub fn get_next_event_at (&self, id: u32, pos: MidiTime) -> Option<&LoopEvent> {
        if let Some(collection) = self.per_id.get(&id) {
            match collection.binary_search_by(|v| {
                v.pos.partial_cmp(&pos).unwrap()
            }) {
                Ok(index) => collection.get(index + 1),
                Err(index) => if index > 0 {
                    collection.get(index)
                } else {
                    None
                }
            }
        } else {
            None
        }
    }
}