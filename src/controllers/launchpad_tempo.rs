use midi_connection;

const PROGRAMMER_MODE: (u8, u8) = (17, 0);
const TEMPO_MODE: (u8, u8) = (15, 0);
const SETTINGS_MODE: (u8, u8) = (18, 0);

pub struct LaunchpadTempo {
    _daw_input: midi_connection::ThreadReference,
}

impl LaunchpadTempo {
    pub fn new(daw_port_name: &str) -> Self {
        let mut output = midi_connection::get_shared_output(daw_port_name);
        let mut last_mode: Option<(u8, u8)> = None;

        let daw_input = midi_connection::get_input(daw_port_name, move |_stamp, message| {
            let new_mode = get_mode(message);

            if new_mode.is_some() {
                // in programmer mode, pressing settings button doesn't do anything, however we do get a second page switch message for
                // programmer mode, we can use this quirk to detect pressing settings in programmer mode
                if new_mode == Some(PROGRAMMER_MODE) && last_mode == Some(PROGRAMMER_MODE) {
                    output.send(&get_mode_message(TEMPO_MODE)).unwrap();
                }

                // we can't just switch back when settings mode activated otherwise we'll end up in a loop where the up event will retrigger tempo mode
                // so lets just switch back to programmer mode after changing away from settings
                if last_mode == Some(SETTINGS_MODE) {
                    output.send(&get_mode_message(PROGRAMMER_MODE)).unwrap();
                }
            }

            last_mode = new_mode;
        });

        LaunchpadTempo {
            _daw_input: daw_input,
        }
    }
}

impl ::controllers::Schedulable for LaunchpadTempo {}

fn get_mode(message: &[u8]) -> Option<(u8, u8)> {
    match message {
        [240, 0, 32, 41, 2, 14, 0, mode, page, 0, 247] => Some((mode.clone(), page.clone())),
        _ => None,
    }
}

fn get_mode_message(mode: (u8, u8)) -> Vec<u8> {
    vec![240, 0, 32, 41, 2, 14, 0, mode.0, mode.1, 0, 247]
}
