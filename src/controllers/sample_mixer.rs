use crate::loop_grid::LoopGridParams;
use crate::midi_connection;
use crate::scheduler::ScheduleRange;
use std::sync::{Arc, Mutex};

const SLIDER_CCS: [u8; 8] = [16, 17, 18, 19, 20, 21, 22, 23];
const MUTE_NOTES: [u8; 8] = [61, 62, 63, 64, 65, 66, 67, 68];

const NOTE_ON_STATUS_CH2: u8 = 144 - 1 + 2;
const CC_STATUS_CH2: u8 = 176 - 1 + 2;

const LED_OFF: u8 = 0;
const LED_RED: u8 = 5;
const LED_WHITE: u8 = 127;

pub struct SampleMixer {
    _midi_input: midi_connection::ThreadReference,
    output: midi_connection::SharedMidiOutputConnection,
    controller_output: midi_connection::SharedMidiOutputConnection,
    params: Arc<Mutex<LoopGridParams>>,
    output_channel: u8,
    output_ccs: Vec<u8>,
    activity_channels: Vec<u32>,
    slider_values: Arc<Mutex<[u8; 8]>>,
    muted: Arc<Mutex<[bool; 8]>>,
    flash_ticks: [u8; 8],
    last_lights: [u8; 8],
}

impl SampleMixer {
    pub fn new(
        output: midi_connection::SharedMidiOutputConnection,
        output_channel: u8,
        output_ccs: Vec<u8>,
        activity_channels: Vec<u32>,
        params: Arc<Mutex<LoopGridParams>>,
    ) -> Self {
        assert_eq!(output_ccs.len(), 8, "SampleMixer requires exactly 8 output_ccs");
        assert_eq!(
            activity_channels.len(),
            8,
            "SampleMixer requires exactly 8 activity_channels"
        );

        let controller_output =
            midi_connection::get_shared_output(midi_connection::YAELTEX_PORT_NAME);
        let slider_values = Arc::new(Mutex::new([0; 8]));
        let muted = Arc::new(Mutex::new([false; 8]));

        let input_slider_values = Arc::clone(&slider_values);
        let input_muted = Arc::clone(&muted);
        let mut input_output = output.clone();
        let output_ccs_for_input = output_ccs.clone();

        let midi_input = midi_connection::get_input(midi_connection::YAELTEX_PORT_NAME, move |_stamp, message| match message {
            [status, cc, value] if *status == CC_STATUS_CH2 => {
                if let Some(index) = SLIDER_CCS.iter().position(|mapped_cc| mapped_cc == cc) {
                    input_slider_values.lock().unwrap()[index] = *value;
                    if !input_muted.lock().unwrap()[index] {
                        let out_cc = output_ccs_for_input[index];
                        input_output
                            .send(&[176 - 1 + output_channel, out_cc, *value])
                            .unwrap();
                    }
                }
            }
            [status, note, velocity] if *status == NOTE_ON_STATUS_CH2 => {
                if *velocity > 0 {
                    if let Some(index) = MUTE_NOTES.iter().position(|mapped_note| mapped_note == note) {
                        let muted_now = {
                            let mut muted = input_muted.lock().unwrap();
                            muted[index] = !muted[index];
                            muted[index]
                        };

                        let value = if muted_now {
                            0
                        } else {
                            input_slider_values.lock().unwrap()[index]
                        };
                        let out_cc = output_ccs_for_input[index];
                        input_output
                            .send(&[176 - 1 + output_channel, out_cc, value])
                            .unwrap();
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
            slider_values,
            muted,
            flash_ticks: [0; 8],
            last_lights: [255; 8],
        };

        instance.refresh_lights();
        instance
    }

    fn refresh_lights(&mut self) {
        let muted = *self.muted.lock().unwrap();
        for index in 0..8 {
            let light = if self.flash_ticks[index] > 0 {
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
}

impl ::controllers::Schedulable for SampleMixer {
    fn schedule(&mut self, range: ScheduleRange) {
        if !range.ticked {
            return;
        }

        {
            let params = self.params.lock().unwrap();
            for (index, channel) in self.activity_channels.iter().enumerate() {
                if params.channel_triggered.contains(channel) {
                    self.flash_ticks[index] = 2;
                }
            }
        }

        for flash in &mut self.flash_ticks {
            if *flash > 0 {
                *flash -= 1;
            }
        }

        self.refresh_lights();
    }
}
