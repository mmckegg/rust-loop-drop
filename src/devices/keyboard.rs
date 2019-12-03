use ::midi_connection;

pub struct Keyboard {
    _midi_input: midi_connection::ThreadReference
}

impl Keyboard {
    pub fn new (port_name: &str, main_output: midi_connection::SharedMidiOutputConnection, output_port: u8) -> Self {
        let mut output = main_output.clone();
        let input = midi_connection::get_input(port_name, move |_stamp, message| {
            match message[0] {
                128 | 144 | 160 | 176 | 192 | 208 | 224 => {
                    let mut msg = Vec::from(message);
                    // reassign midi port
                    msg[0] += output_port - 1;
                    output.send(&msg).unwrap();
                },
                _ => ()
            }
        });

        Self {
            _midi_input: input
        }
    }
}
