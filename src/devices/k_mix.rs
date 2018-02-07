use ::midi_connection;

pub struct KMix {
    midi_port: midi_connection::MidiOutputConnection
}

impl KMix {
    pub fn new (port_name: &str) -> Self {
        KMix {
            midi_port: midi_connection::get_output(port_name).unwrap()
        }
    }
}