use ::midi_connection::SharedMidiOutputConnection;
use std::collections::{HashMap, HashSet};

pub struct ThrottledOutput {
    midi_connection: SharedMidiOutputConnection,
    unsent_values: HashMap<(u8, u8), u8>,
    sent_keys: HashSet<(u8, u8)>
}

impl ThrottledOutput {
    pub fn new (midi_connection: SharedMidiOutputConnection) -> Self {
        ThrottledOutput {
            midi_connection,
            unsent_values: HashMap::new(),
            sent_keys: HashSet::new()
        }
    }

    pub fn flush (&mut self) {
        for ((msg, cc), value) in &self.unsent_values {
            self.midi_connection.send(&[*msg, *cc, *value]).unwrap();
        }
        self.unsent_values.clear();
        self.sent_keys.clear();
    }

    pub fn send (&mut self, message: &[u8]) {
        if message.len() == 3 {
            let key = (message[0], message[1]);
            if self.sent_keys.contains(&key) {
                self.unsent_values.insert(key, message[2]);
            } else {
                self.midi_connection.send(message).unwrap();
                self.sent_keys.insert(key);
            }
        } else {
            self.midi_connection.send(message).unwrap();
        }
    }
}