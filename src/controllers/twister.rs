use lfo::Lfo;
use loop_grid_launchpad::LoopGridParams;
use loop_recorder::{LoopEvent, LoopRecorder};
use midi_connection;
use output_value::OutputValue;
use std::sync::mpsc;
use throttled_output::ThrottledOutput;
use trigger_envelope::TriggerEnvelope;
use MidiTime;

use controllers::{float_to_midi, midi_ease_out, midi_to_polar, polar_to_midi, Modulator};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::thread;

use super::midi_to_float;

pub struct Twister {
    _midi_input: midi_connection::ThreadReference,
    tx: mpsc::Sender<TwisterMessage>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum EventSource {
    User,
    Loop,
}

impl Twister {
    pub fn new(
        port_name: &str,
        main_output: midi_connection::SharedMidiOutputConnection,
        main_channel: u8,
        modulators: Vec<Option<Modulator>>,
        params: Arc<Mutex<LoopGridParams>>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        // let clock_sender = clock.sender.clone();
        let control_ids = get_control_ids();

        let channel_offsets = [10, 20, 30, 40, 50, 60, 70, 80];

        let tx_input = tx.clone();
        let tx_feedback = tx.clone();
        let tx_clock = tx.clone();

        let mut output = midi_connection::get_shared_output(port_name);

        let input = midi_connection::get_input(port_name, move |_stamp, message| {
            let control = Control::from_id(message[1] as u32);
            if message[0] == 176 {
                tx_input
                    .send(TwisterMessage::ControlChange(
                        control,
                        OutputValue::On(message[2]),
                        EventSource::User,
                    ))
                    .unwrap();
            } else if message[0] == 177 {
                tx_input
                    .send(TwisterMessage::Recording(control, message[2] > 0))
                    .unwrap();
            } else if message[0] == 179 && message[1] < 4 && message[2] == 127 {
                tx_input
                    .send(TwisterMessage::BankChange(message[1]))
                    .unwrap();
            } else if message[0] == 179
                && (message[1] == 10 || message[1] == 16 || message[1] == 22 || message[1] == 28)
            {
                tx_input
                    .send(TwisterMessage::LeftButton(message[2] > 0))
                    .unwrap();
            } else if message[0] == 179
                && (message[1] == 13 || message[1] == 19 || message[1] == 25 || message[1] == 31)
            {
                tx_input
                    .send(TwisterMessage::RightButton(message[2] > 0))
                    .unwrap();
            }
        });

        thread::spawn(move || {
            let mut recorder = LoopRecorder::new();
            let mut last_pos = MidiTime::zero();
            let mut last_values: HashMap<Control, u8> = HashMap::new();
            let mut record_start_times = HashMap::new();
            let mut loops: HashMap<Control, Loop> = HashMap::new();
            let mut modulators = modulators;
            let mut throttled_main_output = ThrottledOutput::new(main_output);

            let mut current_bank = 0;

            let mut frozen = false;
            let mut cueing = false;
            let mut frozen_values = None;
            let mut frozen_loops: Option<HashMap<Control, Loop>> = None;
            let mut cued_values: Option<HashMap<Control, u8>> = None;
            let mut triggering_channels: HashSet<u32> = HashSet::new();

            let mut lfo_amounts = HashMap::new();
            let mut duck_amounts = HashMap::new();

            let mut lfo = Lfo::new();
            let mut trigger_envelope = TriggerEnvelope::new(0.9, 0.5);

            for channel in 0..8 {
                last_values.insert(Control::ChannelVolume(channel), 80);
                last_values.insert(Control::ChannelReverb(channel), 0);
                last_values.insert(Control::ChannelDelay(channel), 0);
                last_values.insert(Control::ChannelFilterLfoAmount(channel), 64);
                last_values.insert(Control::ChannelFilter(channel), 64);
                last_values.insert(Control::ChannelDuck(channel), 64);
            }

            last_values.insert(Control::Swing, 64);
            last_values.insert(Control::DuckRelease, 64);
            last_values.insert(Control::LfoRate, 64);
            last_values.insert(Control::LfoSkew, 64);

            // default values for modulators
            for (index, modulator) in modulators.iter().enumerate() {
                if let Some(modulator) = modulator {
                    last_values.insert(
                        Control::Modulator(index),
                        match modulator.modulator {
                            ::config::Modulator::Cc(_id, value) => value,
                            ::config::Modulator::PolarCcSwitch { default, .. } => default,
                            ::config::Modulator::MaxCc(_id, max, value) => {
                                float_to_midi(value.min(max) as f64 / max as f64)
                            }
                            ::config::Modulator::PitchBend(value) => polar_to_midi(value),
                        },
                    );
                }
            }

            // update display and send all of the start values on load
            for control in control_ids.keys() {
                tx.send(TwisterMessage::Send(*control)).unwrap();
                tx.send(TwisterMessage::Refresh(*control)).unwrap();
                if let Some(control_id) = control_ids.get(control) {
                    recorder.allocate(*control_id, 50000);
                }
            }

            // enable nemesis pedal!
            // throttled_main_output.send(&[176 + digit_channel - 1, 38, 127]);

            for received in rx {
                match received {
                    TwisterMessage::LeftButton(pressed) | TwisterMessage::RightButton(pressed) => {
                        let mut params = params.lock().unwrap();
                        if pressed {
                            // if already frozen, go into cueing mode
                            // if already in cueing mode, revert back to normal frozen mode
                            if params.cueing {
                                params.cueing = false
                            } else if params.frozen {
                                params.cueing = true
                            } else {
                                params.frozen = true
                            }
                        } else if !cued_values.is_some() {
                            // only leave frozen on button up if not cueing
                            params.frozen = false
                        }
                    }
                    TwisterMessage::BankChange(bank) => {
                        let mut params = params.lock().unwrap();
                        params.bank = bank;
                    }
                    TwisterMessage::ControlChange(control, value, source) => {
                        if let Some(id) = control_ids.get(&control) {
                            let allow = if loops.contains_key(&control) {
                                let item = loops.get(&control).unwrap();
                                (item.offset + item.length) < (last_pos - MidiTime::from_ticks(8))
                            } else {
                                true
                            };

                            if allow {
                                let event = LoopEvent {
                                    id: id.clone(),
                                    value,
                                    pos: last_pos,
                                };

                                tx_feedback
                                    .send(TwisterMessage::Event(event, source))
                                    .unwrap();
                            }
                        }
                    }
                    TwisterMessage::Send(control) => {
                        let last_value = last_values.get(&control).unwrap_or(&0);
                        let value = if let Some(lfo_amount) = lfo_amounts.get(&control) {
                            let lfo_value = lfo.get_value_at(last_pos);
                            if *lfo_amount > 0.0 {
                                // bipolar modulation (CV style)
                                let polar = ((lfo_value * 2.0) - 1.0) * lfo_amount;
                                (*last_value as f64 + (polar * 64.0)).min(127.0).max(0.0) as u8
                            } else {
                                // treat current value as max and multiplier (subtract / sidechain style)
                                let offset: f64 = lfo_value * (*last_value as f64) * lfo_amount;
                                (*last_value as f64 + offset).min(127.0).max(0.0) as u8
                            }
                        } else if let Some(duck_amount) = duck_amounts.get(&control) {
                            let multiplier = 1.0 - trigger_envelope.value() as f64 * duck_amount;
                            (*last_value as f64 * multiplier).min(127.0).max(0.0) as u8
                        } else {
                            *last_value
                        };

                        match control {
                            Control::ChannelVolume(channel) => {
                                let cc =
                                    channel_offsets[channel as usize % channel_offsets.len()] + 0;
                                throttled_main_output.send(&[
                                    176 - 1 + main_channel,
                                    cc as u8,
                                    midi_ease_out(value),
                                ]);
                            }

                            Control::ChannelReverb(channel) => {
                                let cc =
                                    channel_offsets[channel as usize % channel_offsets.len()] + 1;
                                throttled_main_output.send(&[
                                    176 - 1 + main_channel,
                                    cc as u8,
                                    midi_ease_out(value),
                                ]);
                            }
                            Control::ChannelDelay(channel) => {
                                let cc =
                                    channel_offsets[channel as usize % channel_offsets.len()] + 2;
                                throttled_main_output.send(&[
                                    176 - 1 + main_channel,
                                    cc as u8,
                                    midi_ease_out(value),
                                ]);
                            }

                            Control::ChannelFilter(channel) => {
                                let hp_cc =
                                    channel_offsets[channel as usize % channel_offsets.len()] + 3;
                                let lp_cc =
                                    channel_offsets[channel as usize % channel_offsets.len()] + 4;

                                if value > 60 {
                                    throttled_main_output.send(&[
                                        176 - 1 + main_channel,
                                        hp_cc as u8,
                                        (value.max(64) - 64) * 2,
                                    ]);
                                }

                                if value < 70 {
                                    throttled_main_output.send(&[
                                        176 - 1 + main_channel,
                                        lp_cc as u8,
                                        value.min(63) * 2,
                                    ]);
                                }
                            }

                            Control::ChannelDuck(channel) => {
                                duck_amounts
                                    .insert(Control::ChannelVolume(channel), midi_to_float(value));
                            }

                            Control::DuckRelease => {
                                let multiplier = ((midi_to_float(value) / 2.0) * 0.95) + 0.5;
                                trigger_envelope.tick_multiplier = multiplier as f32;
                            }

                            Control::Swing => {
                                let mut params = params.lock().unwrap();
                                let linear_swing = (value as f64 - 64.0) / 64.0;
                                params.swing = if value == 63 || value == 64 {
                                    0.0
                                } else if linear_swing < 0.0 {
                                    -linear_swing.abs().powf(2.0)
                                } else {
                                    linear_swing.powf(2.0)
                                };
                            }
                            Control::LfoRate => {
                                lfo.speed = value;
                            }
                            Control::LfoSkew => {
                                lfo.skew = value;
                            }
                            Control::Modulator(index) => {
                                if let Some(Some(modulator)) = modulators.get_mut(index) {
                                    modulator.send(value);
                                }
                            }
                            Control::ChannelFilterLfoAmount(channel) => {
                                lfo_amounts
                                    .insert(Control::ChannelFilter(channel), midi_to_polar(value));
                            }

                            Control::None => (),
                        }
                    }
                    TwisterMessage::Event(event, source) => {
                        let control = Control::from_id(event.id);
                        let value = event.value.value();

                        if source != EventSource::Loop && !cueing {
                            loops.remove(&control);
                        }

                        if let Some(loops) = &mut cued_values {
                            if source != EventSource::Loop && cueing {
                                loops.insert(control, value);
                            }
                        }

                        if source == EventSource::Loop || !cueing {
                            last_values.insert(control, value);
                        }

                        // suppress updating device with cued values
                        if source == EventSource::Loop || (source == EventSource::User && !cueing) {
                            tx_feedback.send(TwisterMessage::Send(control)).unwrap();
                        }

                        tx_feedback.send(TwisterMessage::Refresh(control)).unwrap();

                        recorder.add(event);
                    }

                    TwisterMessage::Recording(control, recording) => {
                        if recording {
                            record_start_times.insert(control, last_pos);
                        } else {
                            if let Some(pos) = record_start_times.remove(&control) {
                                let loop_length = MidiTime::quantize_length(last_pos - pos);
                                if loop_length < MidiTime::from_ticks(16) {
                                    loops.remove(&control);
                                } else {
                                    loops.insert(
                                        control,
                                        Loop {
                                            offset: last_pos - loop_length,
                                            length: loop_length,
                                        },
                                    );
                                }
                            }
                        }
                    }

                    TwisterMessage::Refresh(control) => {
                        let cued_value = if let Some(cued_values) = &cued_values {
                            if cueing {
                                cued_values.get(&control)
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        let value = *last_values.get(&control).unwrap_or(&0);
                        if let Some(id) = control_ids.get(&control) {
                            output
                                .send(&[176, id.clone() as u8, *cued_value.unwrap_or(&value)])
                                .unwrap();

                            let channel = id / 2 % 8;

                            // MFT animation for currently looping (Channel 6)
                            if cued_value.is_some() {
                                output.send(&[181, id.clone() as u8, 61]).unwrap();
                            // Fast Indicator Pulse
                            } else if cueing {
                                output.send(&[181, id.clone() as u8, 15]).unwrap();
                            // Fast RGB Pulse
                            } else if frozen {
                                output.send(&[181, id.clone() as u8, 59]).unwrap();
                            // Slow Indicator Pulse
                            } else if triggering_channels.contains(&channel) {
                                output.send(&[181, id.clone() as u8, 17]).unwrap();
                            // Turn off indicator (flash)
                            } else if loops.contains_key(&control) {
                                // control has loop
                                output.send(&[181, id.clone() as u8, 13]).unwrap();
                            // Slow RGB Pulse
                            } else {
                                output.send(&[181, id.clone() as u8, 0]).unwrap();
                            }
                        }
                    }

                    TwisterMessage::Schedule { pos, length } => {
                        let mut params = params.lock().unwrap();
                        if params.reset_automation {
                            // HACK: ack reset message from clear all
                            params.reset_automation = false;
                            loops.clear();

                            for control in control_ids.keys() {
                                tx.send(TwisterMessage::Refresh(*control)).unwrap();
                            }
                        }

                        let mut to_refresh = triggering_channels.clone();
                        triggering_channels.clear();

                        for channel in &params.channel_triggered {
                            triggering_channels.insert(*channel);
                            to_refresh.insert(*channel);
                        }

                        params.channel_triggered.clear();

                        trigger_envelope.tick(params.duck_triggered);
                        params.duck_triggered = false;

                        for (control, id) in control_ids.iter() {
                            let channel = id / 2 % 8;
                            if to_refresh.contains(&channel) {
                                tx.send(TwisterMessage::Refresh(*control)).unwrap();
                            }
                        }

                        if current_bank != params.bank {
                            output.send(&[179, params.bank, 127]).unwrap();
                            current_bank = params.bank;
                        }

                        if params.frozen != frozen {
                            frozen = params.frozen;

                            if frozen {
                                frozen_values = Some(last_values.clone());
                                frozen_loops = Some(loops.clone());
                                for control in control_ids.keys() {
                                    tx.send(TwisterMessage::Refresh(*control)).unwrap();
                                }
                            } else {
                                if let Some(frozen_loops) = frozen_loops.take() {
                                    loops = frozen_loops;
                                }
                                if let Some(frozen_values) = frozen_values.take() {
                                    for (control, _) in &control_ids {
                                        if !loops.contains_key(control)
                                            && frozen_values.get(control)
                                                != last_values.get(control)
                                        {
                                            // queue a value send for changed values on next message loop
                                            tx.send(TwisterMessage::Send(*control)).unwrap();
                                        }
                                    }

                                    last_values = frozen_values;
                                }

                                if let Some(values) = cued_values {
                                    for (key, value) in values {
                                        last_values.insert(key, value);
                                        tx.send(TwisterMessage::Send(key)).unwrap();
                                    }
                                }

                                for control in control_ids.keys() {
                                    tx.send(TwisterMessage::Refresh(*control)).unwrap();
                                }

                                cued_values = None;
                            }
                        }

                        if params.cueing != cueing {
                            cueing = params.cueing;
                            if cueing {
                                if !cued_values.is_some() {
                                    cued_values = Some(HashMap::new());
                                }
                            } else {
                                // force refresh to clear out stalled animations by swapping pages
                                output.send(&[179, (params.bank + 1) % 4, 127]).unwrap();
                                output.send(&[179, params.bank, 127]).unwrap();
                            }

                            for control in control_ids.keys() {
                                tx.send(TwisterMessage::Refresh(*control)).unwrap();
                            }
                        }

                        let mut scheduled = HashSet::new();
                        for (control, value) in &loops {
                            let offset = value.offset % value.length;
                            let playback_pos = value.offset + ((pos - offset) % value.length);

                            if let Some(id) = control_ids.get(control) {
                                if let Some(range) = recorder.get_range_for(
                                    id.clone(),
                                    playback_pos,
                                    playback_pos + length,
                                ) {
                                    for event in range {
                                        tx_feedback
                                            .send(TwisterMessage::Event(
                                                event.clone(),
                                                EventSource::Loop,
                                            ))
                                            .unwrap();
                                        scheduled.insert(control);
                                    }
                                }
                            }
                        }

                        let mut to_refresh = HashSet::new();

                        for (control, value) in &lfo_amounts {
                            if value != &0.0 {
                                to_refresh.insert(control);
                            }
                        }

                        for (control, value) in &duck_amounts {
                            // only schedule ducks if enabled and trigger has value in midi resolution
                            if value > &0.0 && float_to_midi(trigger_envelope.value() as f64) > 0 {
                                to_refresh.insert(control);
                            }
                        }

                        for control in to_refresh {
                            tx_feedback.send(TwisterMessage::Send(*control)).unwrap();
                        }

                        last_pos = pos;

                        throttled_main_output.flush();
                    }
                }
            }
        });

        Twister {
            _midi_input: input,
            tx: tx_clock,
        }
    }
}

impl ::controllers::Schedulable for Twister {
    fn schedule(&mut self, pos: MidiTime, length: MidiTime) {
        self.tx
            .send(TwisterMessage::Schedule { pos, length })
            .unwrap();
    }
}

#[derive(Debug, Clone)]
enum TwisterMessage {
    ControlChange(Control, OutputValue, EventSource),
    BankChange(u8),
    Event(LoopEvent, EventSource),
    Send(Control),
    Refresh(Control),
    Recording(Control, bool),
    LeftButton(bool),
    RightButton(bool),
    Schedule { pos: MidiTime, length: MidiTime },
}

#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
enum Control {
    ChannelVolume(u32),
    ChannelFilter(u32),

    ChannelReverb(u32),
    ChannelDelay(u32),

    ChannelFilterLfoAmount(u32),
    ChannelDuck(u32),

    Modulator(usize),

    DuckRelease,
    Swing,

    LfoRate,
    LfoSkew,

    None,
}

#[derive(Debug, Clone)]
struct Loop {
    offset: MidiTime,
    length: MidiTime,
}

impl Control {
    fn from_id(id: u32) -> Control {
        let page = id / 16;
        let control = id % 2;
        let channel = (id % 16) / 2;

        match (page, channel, control) {
            // Bank A
            (0, channel, 0) => Control::ChannelVolume(channel),
            (0, channel, 1) => Control::ChannelFilter(channel),

            // Bank B
            (1, 7, control) => Control::Modulator((7 * 2 + control) as usize),
            (1, channel, 0) => Control::ChannelReverb(channel),
            (1, channel, 1) => Control::ChannelDelay(channel),

            // Bank C
            (2, 0, 0) => Control::DuckRelease,
            (2, 0, 1) => Control::LfoRate,
            (2, channel, 0) => Control::ChannelDuck(channel),
            (2, channel, 1) => Control::ChannelFilterLfoAmount(channel),

            // PARAMS
            (3, 7, 0) => Control::Swing,
            (3, 7, 1) => Control::LfoSkew,
            (3, channel, control) => Control::Modulator((channel * 2 + control) as usize),

            _ => Control::None,
        }
    }
}

fn get_control_ids() -> HashMap<Control, u32> {
    let mut result = HashMap::new();
    for id in 0..64 {
        let control = Control::from_id(id);
        if control != Control::None {
            result.insert(control, id);
        }
    }
    result
}
