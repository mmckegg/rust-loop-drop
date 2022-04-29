use loop_grid_launchpad::LoopGridParams;
use loop_recorder::{LoopEvent, LoopRecorder};
use midi_connection;
use output_value::OutputValue;
use std::sync::mpsc;
use MidiTime;

use controllers::{float_to_midi, midi_to_float, polar_to_midi, Modulator};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::thread;

pub struct ModTwister {
    _midi_input: midi_connection::ThreadReference,
    tx: mpsc::Sender<ModTwisterMessage>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum EventSource {
    User,
    Loop,
}

impl ModTwister {
    pub fn new(
        port_name: &str,
        modulators: Vec<Modulator>,
        params: Arc<Mutex<LoopGridParams>>,
        continuously_send: Vec<usize>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        // let clock_sender = clock.sender.clone();
        let control_ids = get_control_ids();

        let tx_input = tx.clone();
        let tx_feedback = tx.clone();
        let tx_clock = tx.clone();
        let mut continuously_send_step = 0;

        let mut output = midi_connection::get_shared_output(port_name);

        let input = midi_connection::get_input(port_name, move |_stamp, message| {
            let control = Control::from_id(message[1] as usize);
            if message[0] == 176 {
                tx_input
                    .send(ModTwisterMessage::ControlChange(
                        control,
                        OutputValue::On(message[2]),
                        EventSource::User,
                    ))
                    .unwrap();
            } else if message[0] == 177 {
                tx_input
                    .send(ModTwisterMessage::Recording(control, message[2] > 0))
                    .unwrap();
            } else if message[0] == 179 && message[1] < 4 && message[2] == 127 {
                tx_input
                    .send(ModTwisterMessage::BankChange(message[1]))
                    .unwrap();
            } else if message[0] == 179
                && (message[1] == 10 || message[1] == 16 || message[1] == 22 || message[1] == 28)
            {
                tx_input
                    .send(ModTwisterMessage::LeftButton(message[2] > 0))
                    .unwrap();
            } else if message[0] == 179
                && (message[1] == 13 || message[1] == 19 || message[1] == 25 || message[1] == 31)
            {
                tx_input
                    .send(ModTwisterMessage::RightButton(message[2] > 0))
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

            let mut current_bank = 0;

            let mut frozen = false;
            let mut cueing = false;
            let mut frozen_values: Option<HashMap<Control, u8>> = None;
            let mut frozen_loops: Option<HashMap<Control, Loop>> = None;
            let mut cued_values: Option<HashMap<Control, u8>> = None;

            // default values for modulators
            for (index, modulator) in modulators.iter().enumerate() {
                match modulator {
                    Modulator::MidiModulator(instance) => {
                        last_values.insert(
                            Control::Modulator(index),
                            match instance.modulator {
                                ::config::Modulator::Cc(_id, value) => value,
                                ::config::Modulator::InvertCc(_id, value) => value,
                                ::config::Modulator::InvertMaxCc(_id, max, value) => {
                                    float_to_midi(value.min(max) as f64 / max as f64)
                                }
                                ::config::Modulator::PolarCcSwitch { default, .. } => default,
                                ::config::Modulator::MaxCc(_id, max, value) => {
                                    float_to_midi(value.min(max) as f64 / max as f64)
                                }
                                ::config::Modulator::PitchBend(value) => polar_to_midi(value),
                                ::config::Modulator::PositivePitchBend(value) => {
                                    polar_to_midi(value)
                                }
                            },
                        );
                    }
                    Modulator::DuckDecay(default) => {
                        last_values.insert(Control::Modulator(index), *default);
                    }
                    Modulator::Swing(default) => {
                        last_values.insert(Control::Modulator(index), *default);
                    }
                    Modulator::None => (),
                }
            }

            // update display and send all of the start values on load
            for control in control_ids.keys() {
                tx.send(ModTwisterMessage::Send(*control)).unwrap();
                tx.send(ModTwisterMessage::Refresh(*control)).unwrap();
                if let Some(control_id) = control_ids.get(control) {
                    recorder.allocate(*control_id as u32, 50000);
                }
            }

            for received in rx {
                match received {
                    ModTwisterMessage::LeftButton(pressed)
                    | ModTwisterMessage::RightButton(pressed) => {
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
                    ModTwisterMessage::BankChange(bank) => {
                        let mut params = params.lock().unwrap();
                        params.bank = bank;
                    }
                    ModTwisterMessage::ControlChange(control, value, source) => {
                        if let Some(id) = control_ids.get(&control) {
                            let allow = if loops.contains_key(&control) {
                                let item = loops.get(&control).unwrap();
                                (item.offset + item.length) < (last_pos - MidiTime::from_ticks(8))
                            } else {
                                true
                            };

                            if allow {
                                let event = LoopEvent {
                                    id: *id as u32,
                                    value,
                                    pos: last_pos,
                                };

                                tx_feedback
                                    .send(ModTwisterMessage::Event(event, source))
                                    .unwrap();
                            }
                        }
                    }
                    ModTwisterMessage::Send(control) => {
                        let last_value = last_values.get(&control).unwrap_or(&0);
                        let value = last_value;

                        match control {
                            Control::Modulator(index) => {
                                match modulators.get_mut(index).unwrap_or(&mut Modulator::None) {
                                    Modulator::None => (),
                                    Modulator::MidiModulator(instance) => {
                                        instance.send(*value);
                                    }
                                    Modulator::DuckDecay(..) => {
                                        let mut params = params.lock().unwrap();
                                        let multiplier = midi_to_float(*value) * 0.96;
                                        params.duck_tick_multiplier = multiplier;
                                    }
                                    Modulator::Swing(..) => {
                                        let mut params = params.lock().unwrap();
                                        let value = midi_to_float(*value) * 0.5;
                                        params.swing = value;
                                    }
                                }
                            }

                            Control::None => (),
                        }
                    }
                    ModTwisterMessage::Event(event, source) => {
                        let control = Control::from_id(event.id as usize);
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
                            tx_feedback.send(ModTwisterMessage::Send(control)).unwrap();
                        }

                        tx_feedback
                            .send(ModTwisterMessage::Refresh(control))
                            .unwrap();

                        recorder.add(event);
                    }

                    ModTwisterMessage::Recording(control, recording) => {
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

                    ModTwisterMessage::Refresh(control) => {
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
                            } else if loops.contains_key(&control) {
                                // control has loop
                                output.send(&[181, id.clone() as u8, 13]).unwrap();
                            // Slow RGB Pulse
                            } else {
                                output.send(&[181, id.clone() as u8, 0]).unwrap();
                            }
                        }
                    }

                    ModTwisterMessage::Schedule { pos, length } => {
                        let mut params = params.lock().unwrap();
                        if params.reset_automation {
                            // HACK: ack reset message from clear all
                            params.reset_automation = false;
                            loops.clear();

                            for control in control_ids.keys() {
                                tx.send(ModTwisterMessage::Refresh(*control)).unwrap();
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
                                    tx.send(ModTwisterMessage::Refresh(*control)).unwrap();
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
                                            tx.send(ModTwisterMessage::Send(*control)).unwrap();
                                        }
                                    }

                                    last_values = frozen_values;
                                }

                                if let Some(values) = cued_values {
                                    for (key, value) in values {
                                        last_values.insert(key, value);
                                        tx.send(ModTwisterMessage::Send(key)).unwrap();
                                    }
                                }

                                for control in control_ids.keys() {
                                    tx.send(ModTwisterMessage::Refresh(*control)).unwrap();
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
                                tx.send(ModTwisterMessage::Refresh(*control)).unwrap();
                            }
                        }

                        let mut scheduled = HashSet::new();
                        for (control, value) in &loops {
                            let offset = value.offset % value.length;
                            let playback_pos = value.offset + ((pos - offset) % value.length);

                            if let Some(id) = control_ids.get(control) {
                                if let Some(range) = recorder.get_range_for(
                                    *id as u32,
                                    playback_pos,
                                    playback_pos + length,
                                ) {
                                    for event in range {
                                        tx_feedback
                                            .send(ModTwisterMessage::Event(
                                                event.clone(),
                                                EventSource::Loop,
                                            ))
                                            .unwrap();
                                        scheduled.insert(control);
                                    }
                                }
                            }
                        }

                        // to avoid overwhelming the midi bus, only send one value per tick
                        if continuously_send.len() > 0 {
                            let control =
                                Control::Modulator(continuously_send[continuously_send_step]);

                            if !scheduled.contains(&control) {
                                tx_feedback.send(ModTwisterMessage::Send(control)).unwrap();
                            }
                            continuously_send_step =
                                (continuously_send_step + 1) % continuously_send.len();
                        }

                        last_pos = pos;
                    }
                }
            }
        });

        ModTwister {
            _midi_input: input,
            tx: tx_clock,
        }
    }
}

impl ::controllers::Schedulable for ModTwister {
    fn schedule(&mut self, pos: MidiTime, length: MidiTime) {
        self.tx
            .send(ModTwisterMessage::Schedule { pos, length })
            .unwrap();
    }
}

#[derive(Debug, Clone)]
enum ModTwisterMessage {
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
    Modulator(usize),
    None,
}

#[derive(Debug, Clone)]
struct Loop {
    offset: MidiTime,
    length: MidiTime,
}

impl Control {
    fn from_id(id: usize) -> Control {
        Control::Modulator(id)
    }
}

fn get_control_ids() -> HashMap<Control, usize> {
    let mut result = HashMap::new();
    for id in 0..64 {
        let control = Control::from_id(id);
        if control != Control::None {
            result.insert(control, id);
        }
    }
    result
}
