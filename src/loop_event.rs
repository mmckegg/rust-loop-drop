use midi_time::MidiTime;
use output_value::OutputValue;
use std::cmp::Ordering;

#[derive(Eq, Debug, Copy, Clone)]
pub struct LoopEvent {
    pub value: OutputValue,
    pub pos: MidiTime,
    pub id: u32,
}

impl LoopEvent {
    pub fn is_on(&self) -> bool {
        self.value.is_on()
    }

    pub fn with_pos(&self, new_pos: MidiTime) -> LoopEvent {
        LoopEvent {
            id: self.id,
            value: self.value.clone(),
            pos: new_pos,
        }
    }

    pub fn insert_into(self, target: &mut Vec<LoopEvent>) {
        match target.binary_search_by(|v| v.cmp(&self)) {
            Ok(index) => {
                target.push(self);
                // swap_remove removes at index and puts last item in its place
                target.swap_remove(index);
            }
            Err(index) => target.insert(index, self),
        };
    }

    pub fn range<'a>(
        collection: &'a [LoopEvent],
        start_pos: MidiTime,
        end_pos: MidiTime,
    ) -> &'a [LoopEvent] {
        let start_index = match collection.binary_search_by(|v| {
            if v.pos < start_pos {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }) {
            Ok(index) | Err(index) => index,
        };

        let end_index = match collection.binary_search_by(|v| {
            if v.pos < end_pos {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }) {
            Ok(index) | Err(index) => index,
        };

        &collection[start_index..start_index.max(end_index)]
    }

    pub fn at<'a>(collection: &'a [LoopEvent], pos: MidiTime) -> Option<&'a LoopEvent> {
        match collection.binary_search_by(|v| v.pos.partial_cmp(&pos).unwrap()) {
            Ok(index) => collection.get(index),
            Err(index) => {
                if index > 0 {
                    collection.get(index - 1)
                } else {
                    None
                }
            }
        }
    }
}

impl Ord for LoopEvent {
    fn cmp(&self, other: &LoopEvent) -> Ordering {
        // Some(self.cmp(other))
        let value = self.pos.cmp(&other.pos);
        if self.eq(other) {
            // replace the item if same type,
            Ordering::Equal
        } else if value == Ordering::Equal {
            // or insert after if different (but same position)

            // insert ons after offs (by defining off after on in OutputValue)
            let cmp = other.value.cmp(&self.value);
            match cmp {
                Ordering::Equal => self.id.cmp(&other.id),
                _ => cmp,
            }
        } else {
            value
        }
    }
}

impl PartialOrd for LoopEvent {
    fn partial_cmp(&self, other: &LoopEvent) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for LoopEvent {
    fn eq(&self, other: &LoopEvent) -> bool {
        self.pos == other.pos && self.value == other.value && self.id == other.id
    }
}
