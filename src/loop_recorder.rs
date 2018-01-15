use std::cmp::Ordering;

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum EventType {
    On,
    Off
}

#[derive(Debug)]
pub struct LoopEvent {
    pub event_type: EventType,
    pub pos: f64,
    pub id: u32
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
        self.pos == other.pos && self.event_type == other.event_type && self.id == other.id
    }
}

pub struct LoopRecorder {
    history: Vec<LoopEvent>
}

impl LoopRecorder {
    pub fn new () -> Self {
        Self {
            history: Vec::new()
        }
    }

    pub fn add (&mut self, event: LoopEvent) {
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

    pub fn get_range (&self, start_pos: f64, end_pos: f64) -> &[LoopEvent] {
        let start_index = match self.history.binary_search_by(|v| {
            v.pos.partial_cmp(&start_pos).expect("Cannot compare events (NaN?)")
        }) {
            Ok(index) => index,
            Err(index) => index
        };

        let end_index = match self.history.binary_search_by(|v| {
            v.pos.partial_cmp(&(end_pos + 0.000000001)).expect("Cannot compare events (NaN?)")
        }) {
            Ok(index) => index,
            Err(index) => index
        };

        &self.history[start_index..end_index]
    }

    pub fn get_event_at (&self, id: u32, pos: f64) -> Option<&LoopEvent> {
        match self.get_event_index_at_pos(pos) {
            Some(index) => {
                // walk back from index (including index) to check if the event matches ID
                let to = self.history.len().min(index + 1);
                for i in (0..to).rev() {
                    let event = &self.history[i];
                    if event.id == id {
                        return Some(&event);
                    }
                }

                // can't find any so return none
                None
            },
            None => None
        }
    }

    pub fn get_event_index_at_pos (&self, pos: f64) -> Option<usize> {
        match self.history.binary_search_by(|v| {
            v.pos.partial_cmp(&pos).unwrap()
        }) {
            Ok(index) => Some(index),
            Err(index) => if index > 0 {
                Some(index - 1)
            } else {
                None
            }
        }
    }
}