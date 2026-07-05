use crate::controllers::float_to_midi;
use crate::loop_grid::LoopGridParams;
use crate::midi_connection;
use crate::scheduler::ScheduleRange;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const SLIDER_CCS: [u8; 8] = [16, 17, 18, 19, 20, 21, 22, 23];
const MUTE_NOTES: [u8; 8] = [61, 62, 63, 64, 65, 66, 67, 68];

const NOTE_ON_STATUS_CH2: u8 = 144 - 1 + 2;
const NOTE_OFF_STATUS_CH2: u8 = 128 - 1 + 2;
const CC_STATUS_CH2: u8 = 176 - 1 + 2;

const LED_OFF: u8 = 0;
const LED_RED: u8 = 1;
const LED_WHITE: u8 = 127;
const LED_ON_INTENSITY: u8 = 127;
const EXPRESSION_CURVE_EXPONENT: f64 = 0.75;
const BULK_DIGITAL_FEEDBACK_FRAME_REQUEST: u8 = 0x1D;
const BULK_DIGITAL_FEEDBACK_FRAME_FLAGS: u8 = 0x00;
const BULK_FEEDBACK_READY_POLL_INTERVAL_MS: u64 = 100;
const BULK_FEEDBACK_READY_POLL_MAX_ATTEMPTS: usize = 50;
const BULK_FEEDBACK_READY_POLL: [u8; 9] = [0xF0, 0x79, 0x74, 0x78, 0x00, 0x00, 0x00, 0x1E, 0xF7];
const BULK_FEEDBACK_READY_RESPONSE: [u8; 9] =
    [0xF0, 0x79, 0x74, 0x78, 0x01, 0x00, 0x00, 0x1E, 0xF7];
const BULK_DIGITAL_FEEDBACK_FRAME_PREFIX: [u8; 10] = [
    0xF0,
    0x79,
    0x74,
    0x78,
    0x00,
    0x01,
    0x00,
    BULK_DIGITAL_FEEDBACK_FRAME_REQUEST,
    BULK_DIGITAL_FEEDBACK_FRAME_FLAGS,
    0x00,
];
const MUTE_DIGITAL_INDICES: [u16; 8] = [16, 17, 18, 19, 20, 21, 22, 23];

pub struct SampleMixerState {
    pub sliders: [u8; 8],
    pub slider_seen: [bool; 8],
    pub multipliers: [u8; 8],
    pub muted: [bool; 8],
    pub mute_held: [bool; 8],
    pub mute_latched: [bool; 8],
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
            mute_held: [false; 8],
            mute_latched: [false; 8],
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
    bulk_feedback_ready: Arc<AtomicBool>,
    was_bulk_feedback_ready: bool,
    last_lights: [u8; 8],
}

fn expression_gain(value: u8) -> f64 {
    (value as f64 / 127.0).powf(EXPRESSION_CURVE_EXPONENT)
}

fn send_digital_feedback_frame(
    output: &mut midi_connection::SharedMidiOutputConnection,
    records: &[(u16, u8, u8)],
) {
    if records.is_empty() {
        return;
    }

    let mut message =
        Vec::with_capacity(BULK_DIGITAL_FEEDBACK_FRAME_PREFIX.len() + records.len() * 4 + 1);
    message.extend_from_slice(&BULK_DIGITAL_FEEDBACK_FRAME_PREFIX);
    message[9] = records.len() as u8;

    for (index, color, intensity) in records {
        message.push((index & 0x7F) as u8);
        message.push(((index >> 7) & 0x7F) as u8);
        message.push(*color);
        message.push(*intensity);
    }

    message.push(0xF7);
    let _ = output.send(&message);
}

fn release_mute(
    index: usize,
    state: &Arc<Mutex<SampleMixerState>>,
    params: &Arc<Mutex<LoopGridParams>>,
) {
    let latch_on_release = params.lock().unwrap().select_held;
    let mut state = state.lock().unwrap();

    if !state.mute_held[index] {
        return;
    }

    state.mute_held[index] = false;
    state.mute_latched[index] = latch_on_release;
    state.muted[index] = state.mute_latched[index];
    state.dirty[index] = true;
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
        assert_eq!(
            output_ccs.len(),
            8,
            "SampleMixer requires exactly 8 output_ccs"
        );
        assert_eq!(
            activity_channels.len(),
            8,
            "SampleMixer requires exactly 8 activity_channels"
        );

        let mut controller_output =
            midi_connection::get_shared_output(midi_connection::YAELTEX_PORT_NAME);
        let bulk_feedback_ready = Arc::new(AtomicBool::new(false));
        let bulk_feedback_ready_polling = Arc::new(AtomicBool::new(false));
        let bulk_feedback_ready_on_connect = bulk_feedback_ready.clone();
        let bulk_feedback_ready_polling_on_connect = bulk_feedback_ready_polling.clone();
        let feedback_poll = controller_output.clone();
        controller_output.on_connect(move |_| {
            bulk_feedback_ready_on_connect.store(false, Ordering::SeqCst);
            if bulk_feedback_ready_polling_on_connect.swap(true, Ordering::SeqCst) {
                return;
            }

            let bulk_feedback_ready_polling = bulk_feedback_ready_polling_on_connect.clone();
            let mut feedback_poll = feedback_poll.clone();
            thread::spawn(move || {
                for _ in 0..BULK_FEEDBACK_READY_POLL_MAX_ATTEMPTS {
                    if !bulk_feedback_ready_polling.load(Ordering::SeqCst) {
                        return;
                    }
                    let _ = feedback_poll.send(&BULK_FEEDBACK_READY_POLL);
                    thread::sleep(Duration::from_millis(BULK_FEEDBACK_READY_POLL_INTERVAL_MS));
                }
                bulk_feedback_ready_polling.store(false, Ordering::SeqCst);
                eprintln!("[WARN] Timed out waiting for Yaeltex bulk feedback ready response");
            });
        });

        let input_state = Arc::clone(&state);
        let input_params = Arc::clone(&params);
        let bulk_feedback_ready_on_input = bulk_feedback_ready.clone();
        let bulk_feedback_ready_polling_on_input = bulk_feedback_ready_polling.clone();

        let midi_input = midi_connection::get_input(
            midi_connection::YAELTEX_PORT_NAME,
            move |_stamp, message| match message {
                message if message == BULK_FEEDBACK_READY_RESPONSE.as_slice() => {
                    bulk_feedback_ready_polling_on_input.store(false, Ordering::SeqCst);
                    bulk_feedback_ready_on_input.store(true, Ordering::SeqCst);
                }
                [status, cc, value] if *status == CC_STATUS_CH2 => {
                    if let Some(index) = SLIDER_CCS.iter().position(|mapped_cc| mapped_cc == cc) {
                        let mut state = input_state.lock().unwrap();
                        state.sliders[index] = *value;
                        state.slider_seen[index] = true;
                        state.dirty[index] = true;
                    }
                }
                [status, note, velocity] if *status == NOTE_ON_STATUS_CH2 => {
                    if let Some(index) = MUTE_NOTES
                        .iter()
                        .position(|mapped_note| mapped_note == note)
                    {
                        if *velocity > 0 {
                            let mut state = input_state.lock().unwrap();
                            state.mute_held[index] = true;
                            state.muted[index] = true;
                            state.dirty[index] = true;
                        } else {
                            release_mute(index, &input_state, &input_params);
                        }
                    }
                }
                [status, note, _velocity] if *status == NOTE_OFF_STATUS_CH2 => {
                    if let Some(index) = MUTE_NOTES
                        .iter()
                        .position(|mapped_note| mapped_note == note)
                    {
                        release_mute(index, &input_state, &input_params);
                    }
                }
                _ => {}
            },
        );

        let mut instance = Self {
            _midi_input: midi_input,
            output,
            controller_output,
            params,
            output_channel,
            output_ccs,
            activity_channels,
            state,
            bulk_feedback_ready,
            was_bulk_feedback_ready: false,
            last_lights: [255; 8],
        };

        instance.refresh_lights();
        instance
    }

    fn refresh_lights(&mut self) {
        let bulk_feedback_ready = self.bulk_feedback_ready.load(Ordering::SeqCst);
        if !bulk_feedback_ready {
            if self.was_bulk_feedback_ready {
                self.last_lights = [255; 8];
                self.was_bulk_feedback_ready = false;
            }
            return;
        }

        if !self.was_bulk_feedback_ready {
            self.last_lights = [255; 8];
            self.was_bulk_feedback_ready = true;
        }

        let now = Instant::now();
        let muted = self.state.lock().unwrap().muted;
        let params = self.params.lock().unwrap();
        let mut records = Vec::new();

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
                records.push((
                    MUTE_DIGITAL_INDICES[index],
                    light,
                    if light == LED_OFF {
                        0
                    } else {
                        LED_ON_INTENSITY
                    },
                ));
                self.last_lights[index] = light;
            }
        }

        send_digital_feedback_frame(&mut self.controller_output, &records);
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
