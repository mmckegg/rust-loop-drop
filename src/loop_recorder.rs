pub use loop_event::LoopEvent;
use midi_time::MidiTime;
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::collections::HashMap;

pub struct LoopRecorder {
    per_id: HashMap<u32, Vec<LoopEvent>>,
}

impl LoopRecorder {
    pub fn new() -> Self {
        Self {
            per_id: HashMap::new(),
        }
    }

    pub fn allocate(&mut self, id: u32, capacity: usize) {
        match self.per_id.entry(id) {
            Vacant(entry) => {
                entry.insert(Vec::with_capacity(capacity));
            }
            Occupied(entry) => {
                entry.into_mut().reserve(capacity);
            }
        }
    }

    pub fn add(&mut self, event: LoopEvent) {
        // record events per slot
        let collection = self.per_id.entry(event.id).or_insert(Vec::new());
        event.insert_into(collection);
    }

    pub fn has_events(&self, id: u32, start_pos: MidiTime, end_pos: MidiTime) -> bool {
        if let Some(events) = self.get_range_for(id, start_pos, end_pos) {
            events.iter().any(|item| item.is_on())
        } else {
            false
        }
    }

    pub fn get_range_for(
        &self,
        id: u32,
        start_pos: MidiTime,
        end_pos: MidiTime,
    ) -> Option<&[LoopEvent]> {
        if let Some(collection) = self.per_id.get(&id) {
            Some(LoopEvent::range(collection, start_pos, end_pos))
        } else {
            None
        }
    }

    pub fn get_event_at(&self, id: u32, pos: MidiTime) -> Option<&LoopEvent> {
        if let Some(collection) = self.per_id.get(&id) {
            LoopEvent::at(collection, pos)
        } else {
            None
        }
    }

    pub fn get_next_event_at(&self, id: u32, pos: MidiTime) -> Option<&LoopEvent> {
        if let Some(collection) = self.per_id.get(&id) {
            match collection.binary_search_by(|v| v.pos.cmp(&pos)) {
                Ok(index) => collection.get(index + 1),
                Err(index) => collection.get(index),
            }
        } else {
            None
        }
    }
}
