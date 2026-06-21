use crate::controllers::float_to_midi;
use crate::loop_grid::LoopGridParams;
use crate::midi_connection;
use crate::scheduler::ScheduleRange;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const SLIDER_CCS: [u8; 8] = [16, 17, 18, 19, 20, 21, 22, 23];
const MUTE_NOTES: [u8; 8] = [61, 62, 63, 64, 65, 66, 67, 68];

const NOTE_ON_STATUS_CH2: u8 = 144 - 1 + 2;
const CC_STATUS_CH2: u8 = 176 - 1 + 2;

const LED_OFF: u8 = 0;
const LED_RED: u8 = 5;
const LED_WHITE: u8 = 127;
const EXPRESSION_CURVE_EXPONENT: f64 = 0.75;

pub struct SampleMixerState {
    pub sliders: [u8; 8],
    pub slider_seen: [bool; 8],
    pub multipliers: [u8; 8],
    pub muted: [bool; 8],
    pub last_sent: [u8; 8],
    pub dirty: [bool; 8],
}

impl SampleMixerState {
    pub fn new() -> Self {
        Self {
            sliders: [0; 8],
            slider_seen: [false; 8],
            multipliers: [127; 8],
            muted: [false; 8],
            last_sent: [255; 8],
            dirty: [false; 8],
        }
    }
}

pub struct SampleMixer {
    _midi_input: midi_connection::ThreadReference,
    output: midi_connection::SharedMidiOutputConnection,
    controller_output: midi_connection::SharedMidiOutputConnection,
    params: Arc<Mutex<LoopGridParams>>,
    output_channel: u8,
    output_ccs: Vec<u8>,
    activity_channels: Vec<u32>,
    state: Arc<Mutex<SampleMixerState>>,
    last_lights: [u8; 8],
}

fn expression_gain(value: u8) -> f64 {
    (value as f64 / 127.0).powf(EXPRESSION_CURVE_EXPONENT)
}

impl SampleMixer {
    pub fn new(
        output: midi_connection::SharedMidiOutputConnection,
        output_channel: u8,
        output_ccs: Vec<u8>,
        activity_channels: Vec<u32>,
        params: Arc<Mutex<LoopGridParams>>,
        state: Arc<Mutex<SampleMixerState>>,
    ) -> Self {
        assert_eq!(output_ccs.len(), 8, "SampleMixer requires exactly 8 output_ccs");
        assert_eq!(
            activity_channels.len(),
            8,
            "SampleMixer requires exactly 8 activity_channels"
        );

        let controller_output =
            midi_connection::get_shared_output(midi_connection::YAELTEX_PORT_NAME);

        let input_state = Arc::clone(&state);

        let midi_input = midi_connection::get_input(midi_connection::YAELTEX_PORT_NAME, move |_stamp, message| match message {
            [status, cc, value] if *status == CC_STATUS_CH2 => {
                if let Some(index) = SLIDER_CCS.iter().position(|mapped_cc| mapped_cc == cc) {
                    let mut state = input_state.lock().unwrap();
                    state.sliders[index] = *value;
                    state.slider_seen[index] = true;
                    state.dirty[index] = true;
                }
            }
            [status, note, velocity] if *status == NOTE_ON_STATUS_CH2 => {
                if *velocity > 0 {
                    if let Some(index) = MUTE_NOTES.iter().position(|mapped_note| mapped_note == note) {
                        let mut state = input_state.lock().unwrap();
                        state.muted[index] = !state.muted[index];
                        state.dirty[index] = true;
                    }
                }
            }
            _ => {}
        });

        let mut instance = Self {
            _midi_input: midi_input,
            output,
            controller_output,
            params,
            output_channel,
            output_ccs,
            activity_channels,
            state,
            last_lights: [255; 8],
        };

        instance.refresh_lights();
        instance
    }

    fn refresh_lights(&mut self) {
        let now = Instant::now();
        let muted = self.state.lock().unwrap().muted;
        let params = self.params.lock().unwrap();
        for index in 0..8 {
            let light = if params
                .activity_flash_until
                .get(&self.activity_channels[index])
                .map(|until| *until > now)
                .unwrap_or(false)
            {
                LED_WHITE
            } else if muted[index] {
                LED_RED
            } else {
                LED_OFF
            };

            if self.last_lights[index] != light {
                self.controller_output
                    .send(&[NOTE_ON_STATUS_CH2, MUTE_NOTES[index], light])
                    .unwrap();
                self.last_lights[index] = light;
            }
        }
    }

    fn refresh_output(&mut self) {
        let mut state = self.state.lock().unwrap();
        for index in 0..8 {
            if !state.dirty[index] {
                continue;
            }

            if !state.slider_seen[index] {
                continue;
            }

            let value = if state.muted[index] {
                0
            } else {
                let slider = state.sliders[index] as f64 / 127.0;
                let multiplier = expression_gain(state.multipliers[index]);
                float_to_midi(slider * multiplier)
            };

            if state.last_sent[index] != value {
                self.output
                    .send(&[176 - 1 + self.output_channel, self.output_ccs[index], value])
                    .unwrap();
                state.last_sent[index] = value;
            }

            state.dirty[index] = false;
        }
    }
}

impl ::controllers::Schedulable for SampleMixer {
    fn schedule(&mut self, range: ScheduleRange) {
        if !range.ticked {
            return;
        }

        self.refresh_output();
        self.refresh_lights();
    }
}
