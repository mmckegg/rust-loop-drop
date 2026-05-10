use crate::config::{BankId, EncoderAssignment, EncoderColor, EncoderConfig, EncoderSlot};
use crate::controllers::{midi_to_float, midi_to_polar, Modulator};
use crate::lfo::Lfo;
use crate::loop_event::LoopEvent;
use crate::loop_grid::LoopGridParams;
use crate::scale::Scale;
use crate::loop_recorder::LoopRecorder;
use crate::midi_connection;
use crate::midi_time::MidiTime;
use crate::output_value::OutputValue;
use crate::scheduler::ScheduleRange;
use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const ROTARY_STATUS: u8 = 176;
const SWITCH_FIRST_CC: u8 = 32;
const RING_FIRST_CC: u8 = 0;
const SWITCH_FEEDBACK_STATUS: u8 = 176;
const RING_FEEDBACK_STATUS: u8 = 176;
const RING_MODE_STATUS: u8 = 176;

include!(concat!(env!("OUT_DIR"), "/controller_modes.rs"));
const FEEDBACK_INTENSITY_CHANNEL: u8 = 15;
const COLOR_OFF: u8 = 0;
const COLOR_WHITE: u8 = 3;
const COLOR_RED: u8 = 5;
const COLOR_PURPLE: u8 = 49;
const ROOT_OVERLAY_MS: u64 = 1200;

pub struct ModulationSurface {
    tx: mpsc::Sender<Message>,
    _midi_input: midi_connection::ThreadReference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Lane {
    Base(EncoderSlot),
    LfoAmount(EncoderSlot),
}

#[derive(Debug, Clone)]
enum Message {
    Turn {
        slot: EncoderSlot,
        value: u8,
    },
    Switch {
        slot: EncoderSlot,
        pressed: bool,
        at: Instant,
    },
    Schedule {
        pos: MidiTime,
        length: MidiTime,
    },
}

#[derive(Debug, Clone)]
struct AutomationLoop {
    offset: MidiTime,
    length: MidiTime,
}

impl ModulationSurface {
    pub fn new(
        encoders: Vec<EncoderConfig>,
        _tap_ms: u64,
        double_tap_ms: u64,
        hold_ms: u64,
        params: Arc<Mutex<LoopGridParams>>,
        scale: Arc<Mutex<Scale>>,
        output_ports: &mut HashMap<String, midi_connection::SharedMidiOutputConnection>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let tx_input = tx.clone();
        let tx_clock = tx.clone();
        let mut feedback =
            midi_connection::get_shared_output(midi_connection::YAELTEX_PORT_NAME);

        let slot_by_rotary = build_rotary_slot_map();
        let slot_by_switch = build_switch_slot_map();

        let input = midi_connection::get_input(midi_connection::YAELTEX_PORT_NAME, move |_stamp, message| {
            match message {
                [status, cc, value] if *status == ROTARY_STATUS => {
                    if let Some(slot) = slot_by_rotary.get(cc) {
                        tx_input
                            .send(Message::Turn {
                                slot: *slot,
                                value: *value,
                            })
                            .unwrap();
                    } else if let Some(slot) = slot_by_switch.get(cc) {
                        tx_input
                            .send(Message::Switch {
                                slot: *slot,
                                pressed: *value > 0,
                                at: Instant::now(),
                            })
                            .unwrap();
                    }
                }
                _ => {}
            }
        });

        let configs: HashMap<EncoderSlot, EncoderConfig> =
            encoders.into_iter().map(|c| (c.slot, c)).collect();
        let mut modulators = build_modulators(&configs, output_ports);

        thread::spawn(move || {
            let mut recorder = LoopRecorder::new();
            let mut loops: HashMap<Lane, AutomationLoop> = HashMap::new();
            let mut recording_started: HashMap<Lane, MidiTime> = HashMap::new();
            let mut last_press_at: HashMap<EncoderSlot, Instant> = HashMap::new();
            let mut held_since: HashMap<EncoderSlot, Instant> = HashMap::new();
            let mut active_record_lane: HashMap<EncoderSlot, Lane> = HashMap::new();
            let mut base_values: HashMap<EncoderSlot, u8> = HashMap::new();
            let mut lfo_amounts: HashMap<EncoderSlot, u8> = HashMap::new();
            let mut last_pos = MidiTime::zero();
            let mut lfo = Lfo::new();
            let mut pulsing = 0.0f64;

            for (slot, config) in &configs {
                base_values.insert(*slot, config.assignment.default_value());
                lfo_amounts.insert(*slot, 64);
                recorder.allocate(lane_id(Lane::Base(*slot)), 50000);
                recorder.allocate(lane_id(Lane::LfoAmount(*slot)), 50000);
            }

            loop {
                let msg = match rx.recv() {
                    Ok(msg) => msg,
                    Err(_) => break,
                };

                match msg {
                    Message::Turn { slot, value } => {
                        let (shift, current_bank) = {
                            let params = params.lock().unwrap();
                            (params.select_held, bank_id_from_u8(params.bank))
                        };
                        let slot = slot_in_current_bank(slot, current_bank);
                        let lane = active_lane(slot, shift, &configs);

                        match lane {
                            Lane::Base(slot) => {
                                base_values.insert(slot, value);
                                loops.remove(&Lane::Base(slot));
                                send_lane_value(
                                    slot,
                                    Lane::Base(slot),
                                    value,
                                    &configs,
                                    &mut modulators,
                                    &mut lfo,
                                    &scale,
                                    &params,
                                );
                                recorder.add(LoopEvent {
                                    id: lane_id(Lane::Base(slot)),
                                    value: OutputValue::On(value),
                                    pos: last_pos,
                                });
                            }
                            Lane::LfoAmount(slot) => {
                                lfo_amounts.insert(slot, value);
                                loops.remove(&Lane::LfoAmount(slot));
                            }
                        }

                        refresh_feedback(
                            &configs,
                            &base_values,
                            &lfo_amounts,
                            &loops,
                            &recording_started,
                            shift,
                            current_bank,
                            pulsing,
                            &mut feedback,
                        );
                    }
                    Message::Switch { slot, pressed, at } => {
                        let (shift, current_bank) = {
                            let params = params.lock().unwrap();
                            (params.select_held, bank_id_from_u8(params.bank))
                        };
                        let slot = slot_in_current_bank(slot, current_bank);
                        let lane = active_lane(slot, shift, &configs);

                        if pressed {
                            held_since.insert(slot, at);
                        } else if let Some(pressed_at) = held_since.remove(&slot) {
                            let held_for = at.duration_since(pressed_at);
                            if held_for >= Duration::from_millis(hold_ms) {
                                if active_record_lane.remove(&slot).is_some() {
                                    if let Some(start) = recording_started.remove(&lane) {
                                        let length = MidiTime::quantize_length(last_pos - start);
                                        if length < MidiTime::from_ticks(16) {
                                            loops.remove(&lane);
                                        } else {
                                            loops.insert(
                                                lane,
                                                AutomationLoop {
                                                    offset: last_pos - length,
                                                    length,
                                                },
                                            );
                                        }
                                    }
                                }
                            } else {
                                let now = at;
                                let double_tap = last_press_at
                                    .get(&slot)
                                    .map(|last| now.duration_since(*last) <= Duration::from_millis(double_tap_ms))
                                    .unwrap_or(false);

                                if double_tap {
                                    reset_lane(
                                        lane,
                                        &configs,
                                        &mut base_values,
                                        &mut lfo_amounts,
                                        &mut loops,
                                        &mut modulators,
                                        &mut lfo,
                                        &scale,
                                        &params,
                                    );
                                } else {
                                    loops.remove(&lane);
                                }
                                last_press_at.insert(slot, now);
                            }
                        }

                        refresh_feedback(
                            &configs,
                            &base_values,
                            &lfo_amounts,
                            &loops,
                            &recording_started,
                            shift,
                            current_bank,
                            pulsing,
                            &mut feedback,
                        );
                    }
                    Message::Schedule { pos, length } => {
                        last_pos = pos;
                        pulsing = ((pos.as_float() / 12.0).sin() + 1.0) * 0.5;

                        let (shift, current_bank) = {
                            let params = params.lock().unwrap();
                            (params.select_held, bank_id_from_u8(params.bank))
                        };

                        for (lane, looper) in loops.clone() {
                            let offset = looper.offset % looper.length;
                            let playback_pos = looper.offset + ((pos - offset) % looper.length);
                            if let Some(range) = recorder.get_range_for(
                                lane_id(lane),
                                playback_pos,
                                playback_pos + length,
                            ) {
                                for event in range {
                                    match lane {
                                        Lane::Base(slot) => {
                                            let value = event.value.value();
                                            base_values.insert(slot, value);
                                            send_lane_value(
                                                slot,
                                                lane,
                                                value,
                                                &configs,
                                                &mut modulators,
                                                &mut lfo,
                                                &scale,
                                                &params,
                                            );
                                        }
                                        Lane::LfoAmount(slot) => {
                                            lfo_amounts.insert(slot, event.value.value());
                                        }
                                    }
                                }
                            }
                        }

                        let now = Instant::now();
                        for (slot, pressed_at) in held_since.clone() {
                            if now.duration_since(pressed_at) >= Duration::from_millis(hold_ms)
                                && !active_record_lane.contains_key(&slot)
                            {
                                let lane = active_lane(slot, shift, &configs);
                                active_record_lane.insert(slot, lane);
                                recording_started.insert(lane, pos);
                            }
                        }

                        refresh_feedback(
                            &configs,
                            &base_values,
                            &lfo_amounts,
                            &loops,
                            &recording_started,
                            shift,
                            current_bank,
                            pulsing,
                            &mut feedback,
                        );
                    }
                }
            }
        });

        Self {
            tx: tx_clock,
            _midi_input: input,
        }
    }
}

impl ::controllers::Schedulable for ModulationSurface {
    fn schedule(&mut self, range: ScheduleRange) {
        if range.ticked {
            self.tx
                .send(Message::Schedule {
                    pos: range.tick_pos,
                    length: MidiTime::tick(),
                })
                .unwrap();
        }
    }
}

fn build_modulators(
    configs: &HashMap<EncoderSlot, EncoderConfig>,
    output_ports: &mut HashMap<String, midi_connection::SharedMidiOutputConnection>,
) -> HashMap<EncoderSlot, Modulator> {
    let mut modulators = HashMap::new();
    for (slot, config) in configs {
        let modulator = match &config.assignment {
            EncoderAssignment::None => Modulator::None,
            EncoderAssignment::MidiCc { output, cc, default } => Modulator::MidiModulator(
                crate::controllers::MidiModulator::new(
                    get_port(output_ports, &output.name),
                    output.channel,
                    crate::config::Modulator::Cc(*cc, *default),
                    None,
                ),
            ),
            EncoderAssignment::InvertMidiCc { output, cc, default } => Modulator::MidiModulator(
                crate::controllers::MidiModulator::new(
                    get_port(output_ports, &output.name),
                    output.channel,
                    crate::config::Modulator::InvertCc(*cc, *default),
                    None,
                ),
            ),
            EncoderAssignment::MaxMidiCc {
                output,
                cc,
                max,
                default,
            } => Modulator::MidiModulator(crate::controllers::MidiModulator::new(
                get_port(output_ports, &output.name),
                output.channel,
                crate::config::Modulator::MaxCc(*cc, *max, *default),
                None,
            )),
            EncoderAssignment::InvertMaxMidiCc {
                output,
                cc,
                max,
                default,
            } => Modulator::MidiModulator(crate::controllers::MidiModulator::new(
                get_port(output_ports, &output.name),
                output.channel,
                crate::config::Modulator::InvertMaxCc(*cc, *max, *default),
                None,
            )),
            EncoderAssignment::PolarCcSwitch {
                output,
                cc_low,
                cc_high,
                cc_switch,
                default,
            } => Modulator::MidiModulator(crate::controllers::MidiModulator::new(
                get_port(output_ports, &output.name),
                output.channel,
                crate::config::Modulator::PolarCcSwitch {
                    cc_low: *cc_low,
                    cc_high: *cc_high,
                    cc_switch: *cc_switch,
                    default: *default,
                },
                None,
            )),
            EncoderAssignment::PitchBend {
                output,
                bipolar,
                default,
            } => Modulator::MidiModulator(crate::controllers::MidiModulator::new(
                get_port(output_ports, &output.name),
                output.channel,
                if *bipolar {
                    crate::config::Modulator::PitchBend(midi_to_polar(*default))
                } else {
                    crate::config::Modulator::PositivePitchBend(midi_to_float(*default))
                },
                None,
            )),
            EncoderAssignment::Aftertouch { output, default } => Modulator::MidiModulator(
                crate::controllers::MidiModulator::new(
                    get_port(output_ports, &output.name),
                    output.channel,
                    crate::config::Modulator::Aftertouch(*default),
                    None,
                ),
            ),
            EncoderAssignment::LfoSpeed { default } => Modulator::LfoSpeed(*default),
            EncoderAssignment::LfoWave { default } => Modulator::LfoWave(*default),
            EncoderAssignment::RootNote { default } => Modulator::RootNote(*default),
        };
        modulators.insert(*slot, modulator);
    }
    modulators
}

fn active_lane(
    slot: EncoderSlot,
    shift: bool,
    configs: &HashMap<EncoderSlot, EncoderConfig>,
) -> Lane {
    if shift
        && configs
            .get(&slot)
            .map(|c| c.assignment.supports_lfo_lane())
            .unwrap_or(false)
    {
        Lane::LfoAmount(slot)
    } else {
        Lane::Base(slot)
    }
}

fn lane_id(lane: Lane) -> u32 {
    match lane {
        Lane::Base(slot) => slot_id(slot),
        Lane::LfoAmount(slot) => 1000 + slot_id(slot),
    }
}

fn slot_id(slot: EncoderSlot) -> u32 {
    match slot {
        EncoderSlot::Fixed { row, col } => (row as u32 - 1) * 8 + (col as u32 - 1),
        EncoderSlot::Banked { bank, row, col } => {
            let bank_index = match bank {
                BankId::A => 0,
                BankId::B => 1,
                BankId::C => 2,
                BankId::D => 3,
            };
            100 + bank_index * 8 + (row as u32 - 1) * 4 + (col as u32 - 1)
        }
    }
}

fn build_rotary_slot_map() -> HashMap<u8, EncoderSlot> {
    let mut map = HashMap::new();
    let fixed = fixed_slots();
    for (i, slot) in fixed.iter().enumerate() {
        map.insert(i as u8, *slot);
    }
    for i in 0..8 {
        map.insert(
            20 + i,
            EncoderSlot::Banked {
                bank: BankId::A,
                row: (i / 4) + 1,
                col: (i % 4) + 1,
            },
        );
    }
    map
}

fn build_switch_slot_map() -> HashMap<u8, EncoderSlot> {
    build_rotary_slot_map()
        .into_iter()
        .map(|(cc, slot)| (cc + SWITCH_FIRST_CC, slot))
        .collect()
}

fn fixed_slots() -> Vec<EncoderSlot> {
    vec![
        EncoderSlot::Fixed { row: 1, col: 5 },
        EncoderSlot::Fixed { row: 1, col: 6 },
        EncoderSlot::Fixed { row: 1, col: 7 },
        EncoderSlot::Fixed { row: 1, col: 8 },
        EncoderSlot::Fixed { row: 2, col: 1 },
        EncoderSlot::Fixed { row: 2, col: 2 },
        EncoderSlot::Fixed { row: 2, col: 3 },
        EncoderSlot::Fixed { row: 2, col: 4 },
        EncoderSlot::Fixed { row: 2, col: 5 },
        EncoderSlot::Fixed { row: 2, col: 6 },
        EncoderSlot::Fixed { row: 2, col: 7 },
        EncoderSlot::Fixed { row: 2, col: 8 },
        EncoderSlot::Fixed { row: 3, col: 1 },
        EncoderSlot::Fixed { row: 3, col: 2 },
        EncoderSlot::Fixed { row: 3, col: 3 },
        EncoderSlot::Fixed { row: 3, col: 4 },
        EncoderSlot::Fixed { row: 3, col: 5 },
        EncoderSlot::Fixed { row: 3, col: 6 },
        EncoderSlot::Fixed { row: 3, col: 7 },
        EncoderSlot::Fixed { row: 3, col: 8 },
    ]
}

fn send_lane_value(
    slot: EncoderSlot,
    lane: Lane,
    value: u8,
    configs: &HashMap<EncoderSlot, EncoderConfig>,
    modulators: &mut HashMap<EncoderSlot, Modulator>,
    lfo: &mut Lfo,
    scale: &Arc<Mutex<Scale>>,
    params: &Arc<Mutex<LoopGridParams>>,
) {
    if let Lane::LfoAmount(_) = lane {
        return;
    }

    match modulators.get_mut(&slot).unwrap_or(&mut Modulator::None) {
        Modulator::None => {}
        Modulator::MidiModulator(instance) => instance.send(value),
        Modulator::LfoSpeed(..) => lfo.speed = value,
        Modulator::LfoWave(..) => {
            lfo.wave = value;
        }
        Modulator::RootNote(..) => {
            let note = root_note_from_value(value);
            scale.lock().unwrap().root = note;
            let mut params = params.lock().unwrap();
            params.root_overlay_note = Some(note);
            params.root_overlay_until = Some(Instant::now() + Duration::from_millis(ROOT_OVERLAY_MS));
        }
    }
}

fn reset_lane(
    lane: Lane,
    configs: &HashMap<EncoderSlot, EncoderConfig>,
    base_values: &mut HashMap<EncoderSlot, u8>,
    lfo_amounts: &mut HashMap<EncoderSlot, u8>,
    loops: &mut HashMap<Lane, AutomationLoop>,
    modulators: &mut HashMap<EncoderSlot, Modulator>,
    lfo: &mut Lfo,
    scale: &Arc<Mutex<Scale>>,
    params: &Arc<Mutex<LoopGridParams>>,
) {
    loops.remove(&lane);
    match lane {
        Lane::Base(slot) => {
            if let Some(config) = configs.get(&slot) {
                let value = config.assignment.default_value();
                base_values.insert(slot, value);
                send_lane_value(slot, lane, value, configs, modulators, lfo, scale, params);
            }
        }
        Lane::LfoAmount(slot) => {
            lfo_amounts.insert(slot, 64);
        }
    }
}

fn refresh_feedback(
    configs: &HashMap<EncoderSlot, EncoderConfig>,
    base_values: &HashMap<EncoderSlot, u8>,
    lfo_amounts: &HashMap<EncoderSlot, u8>,
    loops: &HashMap<Lane, AutomationLoop>,
    recording_started: &HashMap<Lane, MidiTime>,
    shift: bool,
    current_bank: BankId,
    pulse: f64,
    feedback: &mut midi_connection::SharedMidiOutputConnection,
) {
    for (index, slot) in fixed_slots().iter().enumerate() {
        render_slot(
            *slot,
            index as u8,
            configs,
            base_values,
            lfo_amounts,
            loops,
            recording_started,
            shift,
            pulse,
            feedback,
        );
    }

    for i in 0..8 {
        let row = (i / 4) + 1;
        let col = (i % 4) + 1;
        let slot = EncoderSlot::Banked {
            bank: current_bank,
            row,
            col,
        };
        render_slot(
            slot,
            20 + i as u8,
            configs,
            base_values,
            lfo_amounts,
            loops,
            recording_started,
            shift,
            pulse,
            feedback,
        );
    }
}

fn bank_id_from_u8(value: u8) -> BankId {
    match value {
        1 => BankId::B,
        2 => BankId::C,
        3 => BankId::D,
        _ => BankId::A,
    }
}

fn slot_in_current_bank(slot: EncoderSlot, current_bank: BankId) -> EncoderSlot {
    match slot {
        EncoderSlot::Banked { row, col, .. } => EncoderSlot::Banked {
            bank: current_bank,
            row,
            col,
        },
        _ => slot,
    }
}

#[allow(clippy::too_many_arguments)]
fn render_slot(
    slot: EncoderSlot,
    physical_index: u8,
    configs: &HashMap<EncoderSlot, EncoderConfig>,
    base_values: &HashMap<EncoderSlot, u8>,
    lfo_amounts: &HashMap<EncoderSlot, u8>,
    loops: &HashMap<Lane, AutomationLoop>,
    recording_started: &HashMap<Lane, MidiTime>,
    shift: bool,
    pulse: f64,
    feedback: &mut midi_connection::SharedMidiOutputConnection,
) {
    let config = if let Some(config) = configs.get(&slot) {
        config
    } else {
        send_color_feedback(feedback, SWITCH_FEEDBACK_STATUS, SWITCH_FIRST_CC + physical_index, COLOR_OFF, 0);
        send_color_feedback(feedback, RING_FEEDBACK_STATUS, RING_FIRST_CC + physical_index, COLOR_OFF, 0);
        return;
    };

    let active_lane = active_lane(slot, shift, configs);
    let is_recording = recording_started.contains_key(&active_lane);
    let has_loop = loops.contains_key(&active_lane);

    let switch_color = encoder_color_to_midi(config.color);
    let switch_intensity = if has_loop {
        (48.0 + pulse * 79.0) as u8
    } else {
        96
    };

    let ring_color = if is_recording {
        COLOR_RED
    } else if shift && config.assignment.supports_lfo_lane() {
        COLOR_PURPLE
    } else {
        COLOR_WHITE
    };

    let ring_value = if shift && config.assignment.supports_lfo_lane() {
        *lfo_amounts.get(&slot).unwrap_or(&64)
    } else {
        *base_values.get(&slot).unwrap_or(&config.assignment.default_value())
    };

    let ring_intensity = if !shift && config.assignment.supports_lfo_lane() {
        let amount = lfo_amounts.get(&slot).copied().unwrap_or(64);
        let depth = (midi_to_polar(amount)).abs();
        (64.0 + (pulse * depth * 63.0)) as u8
    } else {
        127
    };

    send_color_feedback(
        feedback,
        SWITCH_FEEDBACK_STATUS,
        SWITCH_FIRST_CC + physical_index,
        switch_color,
        switch_intensity,
    );
    send_color_feedback(
        feedback,
        RING_FEEDBACK_STATUS,
        RING_FIRST_CC + physical_index,
        ring_color,
        ring_intensity,
    );
    let ring_mode_value = PHYSICAL_ENCODER_RING_MODES[physical_index as usize];
    let _ = feedback.send(&[RING_MODE_STATUS, RING_FIRST_CC + physical_index, ring_mode_value]);
    let _ = feedback.send(&[RING_FEEDBACK_STATUS, RING_FIRST_CC + physical_index, ring_value]);
}

fn root_note_from_value(value: u8) -> i32 {
    60 + (((value as i32 - 64) * 12) / 63)
}

fn encoder_color_to_midi(color: EncoderColor) -> u8 {
    match color {
        EncoderColor::White => COLOR_WHITE,
        EncoderColor::Yellow => 13,
        EncoderColor::Orange => 9,
        EncoderColor::Blue => 45,
        EncoderColor::Purple => COLOR_PURPLE,
        EncoderColor::Pink => 53,
        EncoderColor::Cyan => 33,
        EncoderColor::Lime => 17,
        EncoderColor::Red => COLOR_RED,
        EncoderColor::Green => 17,
    }
}

fn send_color_feedback(
    feedback: &mut midi_connection::SharedMidiOutputConnection,
    status: u8,
    cc: u8,
    color: u8,
    intensity: u8,
) {
    let _ = feedback.send(&[status, cc, color]);
    let _ = feedback.send(&[176 - 1 + FEEDBACK_INTENSITY_CHANNEL, cc, intensity]);
}

fn get_port(
    ports_lookup: &mut HashMap<String, midi_connection::SharedMidiOutputConnection>,
    port_name: &str,
) -> midi_connection::SharedMidiOutputConnection {
    if !ports_lookup.contains_key(port_name) {
        ports_lookup.insert(
            String::from(port_name),
            midi_connection::get_shared_output(port_name),
        );
    }
    ports_lookup.get(port_name).unwrap().clone()
}
