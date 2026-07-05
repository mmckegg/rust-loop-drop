use crate::config::{
    ActivityHighlight, BankId, EncoderAssignment, EncoderColor, EncoderConfig, EncoderRingType,
    EncoderSlot, LfoMode,
};
use crate::controllers::{midi_to_float, midi_to_polar, Modulator};
use crate::lfo::Lfo;
use crate::loop_event::LoopEvent;
use crate::loop_grid::LoopGridParams;
use crate::loop_recorder::LoopRecorder;
use crate::midi_connection;
use crate::midi_time::MidiTime;
use crate::output_value::OutputValue;
use crate::scale::Scale;
use crate::scheduler::ScheduleRange;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const AUTOMATION_DIM_INTENSITY: u8 = 48;
const AUTOMATION_BRIGHT_INTENSITY: u8 = 127;
const ACTIVITY_FLASH_INTENSITY: u8 = 127;
const SWITCH_IDLE_INTENSITY: u8 = 96;
const RING_IDLE_INTENSITY: u8 = 127;
const LFO_ACTIVE_COLOR_SWAP_INTERVAL_TICKS: i32 = 24;

const ROTARY_STATUS: u8 = 176;
const SWITCH_FIRST_CC: u8 = 32;
const BULK_ENCODER_FEEDBACK_FRAME_REQUEST: u8 = 0x1C;
const BULK_ENCODER_FEEDBACK_FRAME_FLAGS: u8 = 0x00;
const BULK_FEEDBACK_READY_POLL_INTERVAL_MS: u64 = 100;
const BULK_FEEDBACK_READY_POLL_MAX_ATTEMPTS: usize = 50;
const BULK_FEEDBACK_READY_POLL: [u8; 9] = [0xF0, 0x79, 0x74, 0x78, 0x00, 0x00, 0x00, 0x1E, 0xF7];
const BULK_FEEDBACK_READY_RESPONSE: [u8; 9] =
    [0xF0, 0x79, 0x74, 0x78, 0x01, 0x00, 0x00, 0x1E, 0xF7];
const BULK_ENCODER_FEEDBACK_FRAME_PREFIX: [u8; 10] = [
    0xF0,
    0x79,
    0x74,
    0x78,
    0x00,
    0x01,
    0x00,
    BULK_ENCODER_FEEDBACK_FRAME_REQUEST,
    BULK_ENCODER_FEEDBACK_FRAME_FLAGS,
    0x00,
];

const COLOR_OFF: u8 = 0;
const COLOR_WHITE: u8 = 127;
const COLOR_RED: u8 = 1;
const COLOR_PURPLE: u8 = 97;
const ROOT_OVERLAY_MS: u64 = 1200;

pub struct ModulationSurface {
    tx: mpsc::Sender<Message>,
    _midi_input: midi_connection::ThreadReference,
}

#[derive(Clone)]
pub struct ModulationSurfaceShared {
    pub sample_mixer_state: Arc<Mutex<crate::controllers::SampleMixerState>>,
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
        delta: i8,
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
    WaitForBulkFeedbackReady,
    ForceFeedbackRefresh,
}

#[derive(Debug, Clone)]
struct AutomationLoop {
    offset: MidiTime,
    length: MidiTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiFeedback {
    Manual,
    Silent,
}

fn decode_relative_encoder_delta(value: u8) -> Option<i8> {
    let delta = value as i16 - 64;
    if delta == 0 {
        None
    } else {
        Some(delta.clamp(-63, 63) as i8)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SlotFeedback {
    switch_color: u8,
    switch_intensity: u8,
    ring_color: u8,
    ring_intensity: u8,
    ring_mode: u8,
    ring_value: u8,
}

impl ModulationSurface {
    pub fn new(
        encoders: Vec<EncoderConfig>,
        tap_ms: u64,
        double_tap_ms: u64,
        hold_ms: u64,
        params: Arc<Mutex<LoopGridParams>>,
        scale: Arc<Mutex<Scale>>,
        shared: ModulationSurfaceShared,
        output_ports: &mut HashMap<String, midi_connection::SharedMidiOutputConnection>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let tx_input = tx.clone();
        let tx_clock = tx.clone();
        let mut feedback = midi_connection::get_shared_output(midi_connection::YAELTEX_PORT_NAME);
        let bulk_feedback_ready_polling = Arc::new(AtomicBool::new(false));
        let encoder_feedback_ready = Arc::new(AtomicBool::new(false));
        let tx_reconnect = tx.clone();
        let feedback_poll = feedback.clone();
        let bulk_feedback_ready_polling_on_connect = bulk_feedback_ready_polling.clone();
        let encoder_feedback_ready_on_connect = encoder_feedback_ready.clone();
        feedback.on_connect(move |_| {
            encoder_feedback_ready_on_connect.store(false, Ordering::SeqCst);
            let _ = tx_reconnect.send(Message::WaitForBulkFeedbackReady);
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

        let slot_by_rotary = build_rotary_slot_map();
        let slot_by_switch = build_switch_slot_map();
        let bulk_feedback_ready_polling_on_input = bulk_feedback_ready_polling.clone();
        let encoder_feedback_ready_on_input = encoder_feedback_ready.clone();

        let input = midi_connection::get_input(
            midi_connection::YAELTEX_PORT_NAME,
            move |_stamp, message| match message {
                message if message == BULK_FEEDBACK_READY_RESPONSE.as_slice() => {
                    bulk_feedback_ready_polling_on_input.store(false, Ordering::SeqCst);
                    if encoder_feedback_ready_on_input.swap(true, Ordering::SeqCst) {
                        return;
                    }

                    println!("[INFO] Encoder bulk feedback ready");
                    let _ = tx_input.send(Message::ForceFeedbackRefresh);
                }
                [status, cc, value] if *status == ROTARY_STATUS => {
                    if let Some(slot) = slot_by_rotary.get(cc) {
                        if let Some(delta) = decode_relative_encoder_delta(*value) {
                            tx_input.send(Message::Turn { slot: *slot, delta }).unwrap();
                        }
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
            },
        );

        let configs: HashMap<EncoderSlot, EncoderConfig> =
            encoders.into_iter().map(|c| (c.slot, c)).collect();
        let mut modulators = build_modulators(&configs, &shared, output_ports);

        thread::spawn(move || {
            let mut recorder = LoopRecorder::new();
            let mut loops: HashMap<Lane, AutomationLoop> = HashMap::new();
            let mut recording_started: HashMap<Lane, MidiTime> = HashMap::new();
            let mut last_press_at: HashMap<EncoderSlot, Instant> = HashMap::new();
            let mut recent_record_release_at: HashMap<EncoderSlot, Instant> = HashMap::new();
            let mut held_since: HashMap<EncoderSlot, Instant> = HashMap::new();
            let mut active_record_lane: HashMap<EncoderSlot, Lane> = HashMap::new();
            let mut base_values: HashMap<EncoderSlot, u8> = HashMap::new();
            let mut lfo_amounts: HashMap<EncoderSlot, u8> = HashMap::new();
            let mut prepared_values: HashMap<Lane, u8> = HashMap::new();
            let mut last_sent_output: HashMap<EncoderSlot, u8> = HashMap::new();
            let mut last_feedback: HashMap<u8, SlotFeedback> = HashMap::new();
            let mut bulk_feedback_ready = false;
            let mut last_pos = MidiTime::zero();
            let mut lfo = Lfo::new();
            let mut pulsing = 0.0f64;

            for (slot, config) in &configs {
                base_values.insert(*slot, config.assignment.default_value());
                lfo_amounts.insert(*slot, neutral_lfo_amount(config));
                last_sent_output.insert(*slot, 255);
                recorder.allocate(lane_id(Lane::Base(*slot)), 50000);
                recorder.allocate(lane_id(Lane::LfoAmount(*slot)), 50000);
            }

            for (slot, config) in &configs {
                let value = config.assignment.default_value();
                match modulators.get_mut(slot).unwrap_or(&mut Modulator::None) {
                    Modulator::None => {}
                    Modulator::MidiModulator(instance) => {
                        instance.send(value);
                        last_sent_output.insert(*slot, value);
                    }
                    Modulator::LfoSpeed(..) => lfo.speed = value,
                    Modulator::LfoWave(..) => lfo.wave = value,
                    Modulator::RootNote(..) => {
                        scale.lock().unwrap().root = root_note_from_value(value);
                    }
                    Modulator::SampleLevelMultiplier { sample, state } => {
                        let index = *sample as usize;
                        let mut state = state.lock().unwrap();
                        if index < state.multipliers.len() {
                            state.multipliers[index] = value;
                            state.dirty[index] = true;
                            last_sent_output.insert(*slot, value);
                        }
                    }
                }
            }

            loop {
                let msg = match rx.recv() {
                    Ok(msg) => msg,
                    Err(_) => break,
                };

                match msg {
                    Message::Turn { slot, delta } => {
                        let (shift, current_bank, prepare_held) = {
                            let params = params.lock().unwrap();
                            (
                                params.select_held,
                                bank_id_from_u8(params.bank),
                                params.prepare_held,
                            )
                        };
                        let slot = slot_in_current_bank(slot, current_bank);
                        let lane = active_lane(slot, shift, &configs);
                        let release_guard = recent_record_release_at
                            .get(&slot)
                            .map(|last| {
                                Instant::now().duration_since(*last)
                                    <= Duration::from_millis(tap_ms)
                            })
                            .unwrap_or(false);

                        if !release_guard {
                            let value = apply_encoder_delta(
                                lane,
                                delta,
                                &configs,
                                &base_values,
                                &lfo_amounts,
                                &prepared_values,
                            );
                            if prepare_held {
                                prepared_values.insert(lane, value);
                                if let Lane::Base(slot) = lane {
                                    show_manual_internal_overlay(slot, value, &configs, &params);
                                }
                            } else {
                                commit_lane_value(
                                    lane,
                                    value,
                                    last_pos,
                                    &configs,
                                    &mut base_values,
                                    &mut lfo_amounts,
                                    &mut loops,
                                    &mut recorder,
                                    &mut last_sent_output,
                                    &mut modulators,
                                    &mut lfo,
                                    &scale,
                                    &params,
                                    UiFeedback::Manual,
                                );
                            }
                        }

                        if bulk_feedback_ready {
                            refresh_feedback(
                                &configs,
                                &base_values,
                                &lfo_amounts,
                                &prepared_values,
                                &loops,
                                &recording_started,
                                &params,
                                shift,
                                current_bank,
                                pulsing,
                                last_pos,
                                &mut last_feedback,
                                &mut feedback,
                            );
                        }
                    }
                    Message::Switch { slot, pressed, at } => {
                        let (shift, current_bank) = {
                            let params = params.lock().unwrap();
                            (params.select_held, bank_id_from_u8(params.bank))
                        };
                        let slot = slot_in_current_bank(slot, current_bank);
                        let lane = active_lane(slot, shift, &configs);

                        if pressed {
                            if let EncoderAssignment::RootNote { .. } = configs
                                .get(&slot)
                                .map(|c| &c.assignment)
                                .unwrap_or(&EncoderAssignment::None)
                            {
                                let mut params = params.lock().unwrap();
                                params.root_overlay_note = Some(root_note_from_value(
                                    *base_values.get(&slot).unwrap_or(&64),
                                ));
                                params.root_overlay_until =
                                    Some(Instant::now() + Duration::from_millis(ROOT_OVERLAY_MS));
                            }
                            held_since.insert(slot, at);
                            active_record_lane.insert(slot, lane);
                            recording_started.insert(lane, last_pos);
                        } else if let Some(pressed_at) = held_since.remove(&slot) {
                            let held_for = at.duration_since(pressed_at);
                            let recorded_lane = active_record_lane.remove(&slot).unwrap_or(lane);
                            prepared_values.remove(&recorded_lane);

                            if held_for >= Duration::from_millis(hold_ms) {
                                recent_record_release_at.insert(slot, at);
                                if let Some(start) = recording_started.remove(&recorded_lane) {
                                    let length = MidiTime::quantize_length(last_pos - start);
                                    if length < MidiTime::from_ticks(16) {
                                        loops.remove(&recorded_lane);
                                    } else {
                                        loops.insert(
                                            recorded_lane,
                                            AutomationLoop {
                                                offset: last_pos - length,
                                                length,
                                            },
                                        );
                                    }
                                }
                            } else {
                                recording_started.remove(&recorded_lane);

                                let now = at;
                                let release_guard = recent_record_release_at
                                    .get(&slot)
                                    .map(|last| {
                                        now.duration_since(*last) <= Duration::from_millis(tap_ms)
                                    })
                                    .unwrap_or(false);

                                if !release_guard {
                                    let double_tap = last_press_at
                                        .get(&slot)
                                        .map(|last| {
                                            now.duration_since(*last)
                                                <= Duration::from_millis(double_tap_ms)
                                        })
                                        .unwrap_or(false);

                                    let id = match recorded_lane {
                                        Lane::Base(id) | Lane::LfoAmount(id) => id,
                                    };

                                    if double_tap {
                                        reset_lane(
                                            recorded_lane,
                                            &configs,
                                            &mut base_values,
                                            &mut lfo_amounts,
                                            &mut loops,
                                            &mut last_sent_output,
                                            &mut modulators,
                                            &mut lfo,
                                            &scale,
                                            &params,
                                        );
                                        if let Lane::Base(value) = recorded_lane {
                                            // clear both lanes for a double tap on Base
                                            prepared_values.remove(&Lane::LfoAmount(value));
                                            reset_lane(
                                                Lane::LfoAmount(value),
                                                &configs,
                                                &mut base_values,
                                                &mut lfo_amounts,
                                                &mut loops,
                                                &mut last_sent_output,
                                                &mut modulators,
                                                &mut lfo,
                                                &scale,
                                                &params,
                                            );
                                        }
                                    } else {
                                        loops.remove(&recorded_lane);
                                    }
                                    last_press_at.insert(slot, now);
                                }
                            }
                        }

                        if bulk_feedback_ready {
                            refresh_feedback(
                                &configs,
                                &base_values,
                                &lfo_amounts,
                                &prepared_values,
                                &loops,
                                &recording_started,
                                &params,
                                shift,
                                current_bank,
                                pulsing,
                                last_pos,
                                &mut last_feedback,
                                &mut feedback,
                            );
                        }
                    }
                    Message::WaitForBulkFeedbackReady => {
                        bulk_feedback_ready = false;
                        last_feedback.clear();
                    }
                    Message::ForceFeedbackRefresh => {
                        let (shift, current_bank) = {
                            let params = params.lock().unwrap();
                            (params.select_held, bank_id_from_u8(params.bank))
                        };
                        bulk_feedback_ready = true;
                        last_feedback.clear();
                        refresh_feedback(
                            &configs,
                            &base_values,
                            &lfo_amounts,
                            &prepared_values,
                            &loops,
                            &recording_started,
                            &params,
                            shift,
                            current_bank,
                            pulsing,
                            last_pos,
                            &mut last_feedback,
                            &mut feedback,
                        );
                    }
                    Message::Schedule { pos, length } => {
                        last_pos = pos;
                        pulsing = ((pos.as_float() / 12.0).sin() + 1.0) * 0.5;

                        let (shift, current_bank, prepare_held) = {
                            let params = params.lock().unwrap();
                            (
                                params.select_held,
                                bank_id_from_u8(params.bank),
                                params.prepare_held,
                            )
                        };

                        if !prepare_held && !prepared_values.is_empty() {
                            commit_prepared_values(
                                &mut prepared_values,
                                pos,
                                &configs,
                                &mut base_values,
                                &mut lfo_amounts,
                                &mut loops,
                                &mut recorder,
                                &mut last_sent_output,
                                &mut modulators,
                                &mut lfo,
                                &scale,
                                &params,
                            );
                        }

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
                                        }
                                        Lane::LfoAmount(slot) => {
                                            lfo_amounts.insert(slot, event.value.value());
                                        }
                                    }
                                }
                            }
                        }

                        let mut slots_to_update = std::collections::HashSet::new();
                        for lane in loops.keys() {
                            match *lane {
                                Lane::Base(slot) | Lane::LfoAmount(slot) => {
                                    slots_to_update.insert(slot);
                                }
                            }
                        }
                        for (slot, amount) in &lfo_amounts {
                            if let Some(config) = configs.get(slot) {
                                if lfo_amount_is_active(*amount, config) {
                                    slots_to_update.insert(*slot);
                                }
                            }
                        }
                        for slot in slots_to_update {
                            let value = *base_values.get(&slot).unwrap_or(
                                &configs
                                    .get(&slot)
                                    .map(|c| c.assignment.default_value())
                                    .unwrap_or(0),
                            );
                            send_lane_value(
                                slot,
                                Lane::Base(slot),
                                value,
                                pos,
                                &configs,
                                &lfo_amounts,
                                &mut last_sent_output,
                                &mut modulators,
                                &mut lfo,
                                &scale,
                                &params,
                                UiFeedback::Silent,
                            );
                        }

                        if bulk_feedback_ready {
                            refresh_feedback(
                                &configs,
                                &base_values,
                                &lfo_amounts,
                                &prepared_values,
                                &loops,
                                &recording_started,
                                &params,
                                shift,
                                current_bank,
                                pulsing,
                                pos,
                                &mut last_feedback,
                                &mut feedback,
                            );
                        }
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
    shared: &ModulationSurfaceShared,
    output_ports: &mut HashMap<String, midi_connection::SharedMidiOutputConnection>,
) -> HashMap<EncoderSlot, Modulator> {
    let mut modulators = HashMap::new();
    for (slot, config) in configs {
        let modulator = match &config.assignment {
            EncoderAssignment::None => Modulator::None,
            EncoderAssignment::MidiCc {
                output,
                cc,
                default,
            } => Modulator::MidiModulator(crate::controllers::MidiModulator::new(
                get_port(output_ports, &output.name),
                output.channel,
                crate::config::Modulator::Cc(*cc, *default),
                None,
            )),
            EncoderAssignment::InvertMidiCc {
                output,
                cc,
                default,
            } => Modulator::MidiModulator(crate::controllers::MidiModulator::new(
                get_port(output_ports, &output.name),
                output.channel,
                crate::config::Modulator::InvertCc(*cc, *default),
                None,
            )),
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
            EncoderAssignment::Aftertouch { output, default } => {
                Modulator::MidiModulator(crate::controllers::MidiModulator::new(
                    get_port(output_ports, &output.name),
                    output.channel,
                    crate::config::Modulator::Aftertouch(*default),
                    None,
                ))
            }
            EncoderAssignment::LfoSpeed { default } => Modulator::LfoSpeed(*default),
            EncoderAssignment::LfoWave { default } => Modulator::LfoWave(*default),
            EncoderAssignment::RootNote { default } => Modulator::RootNote(*default),
            EncoderAssignment::SampleLevelMultiplier { sample, .. } => {
                Modulator::SampleLevelMultiplier {
                    sample: *sample,
                    state: Arc::clone(&shared.sample_mixer_state),
                }
            }
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

fn show_manual_internal_overlay(
    slot: EncoderSlot,
    value: u8,
    configs: &HashMap<EncoderSlot, EncoderConfig>,
    params: &Arc<Mutex<LoopGridParams>>,
) {
    if let Some(config) = configs.get(&slot) {
        if let EncoderAssignment::RootNote { .. } = config.assignment {
            let note = root_note_from_value(value);
            let mut params = params.lock().unwrap();
            params.root_overlay_note = Some(note);
            params.root_overlay_until =
                Some(Instant::now() + Duration::from_millis(ROOT_OVERLAY_MS));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn commit_prepared_values(
    prepared_values: &mut HashMap<Lane, u8>,
    pos: MidiTime,
    configs: &HashMap<EncoderSlot, EncoderConfig>,
    base_values: &mut HashMap<EncoderSlot, u8>,
    lfo_amounts: &mut HashMap<EncoderSlot, u8>,
    loops: &mut HashMap<Lane, AutomationLoop>,
    recorder: &mut LoopRecorder,
    last_sent_output: &mut HashMap<EncoderSlot, u8>,
    modulators: &mut HashMap<EncoderSlot, Modulator>,
    lfo: &mut Lfo,
    scale: &Arc<Mutex<Scale>>,
    params: &Arc<Mutex<LoopGridParams>>,
) {
    let pending: Vec<(Lane, u8)> = prepared_values.drain().collect();
    for (lane, value) in pending {
        commit_lane_value(
            lane,
            value,
            pos,
            configs,
            base_values,
            lfo_amounts,
            loops,
            recorder,
            last_sent_output,
            modulators,
            lfo,
            scale,
            params,
            UiFeedback::Manual,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn commit_lane_value(
    lane: Lane,
    value: u8,
    pos: MidiTime,
    configs: &HashMap<EncoderSlot, EncoderConfig>,
    base_values: &mut HashMap<EncoderSlot, u8>,
    lfo_amounts: &mut HashMap<EncoderSlot, u8>,
    loops: &mut HashMap<Lane, AutomationLoop>,
    recorder: &mut LoopRecorder,
    last_sent_output: &mut HashMap<EncoderSlot, u8>,
    modulators: &mut HashMap<EncoderSlot, Modulator>,
    lfo: &mut Lfo,
    scale: &Arc<Mutex<Scale>>,
    params: &Arc<Mutex<LoopGridParams>>,
    ui_feedback: UiFeedback,
) {
    match lane {
        Lane::Base(slot) => {
            base_values.insert(slot, value);
            loops.remove(&Lane::Base(slot));
            send_lane_value(
                slot,
                Lane::Base(slot),
                value,
                pos,
                configs,
                lfo_amounts,
                last_sent_output,
                modulators,
                lfo,
                scale,
                params,
                ui_feedback,
            );
            if ui_feedback == UiFeedback::Manual {
                show_manual_internal_overlay(slot, value, configs, params);
            }
            recorder.add(LoopEvent {
                id: lane_id(Lane::Base(slot)),
                value: OutputValue::On(value),
                pos,
            });
        }
        Lane::LfoAmount(slot) => {
            lfo_amounts.insert(slot, value);
            loops.remove(&Lane::LfoAmount(slot));
            let base_value = *base_values.get(&slot).unwrap_or(
                &configs
                    .get(&slot)
                    .map(|c| c.assignment.default_value())
                    .unwrap_or(0),
            );
            send_lane_value(
                slot,
                Lane::Base(slot),
                base_value,
                pos,
                configs,
                lfo_amounts,
                last_sent_output,
                modulators,
                lfo,
                scale,
                params,
                ui_feedback,
            );
            recorder.add(LoopEvent {
                id: lane_id(Lane::LfoAmount(slot)),
                value: OutputValue::On(value),
                pos,
            });
        }
    }
}

fn send_lane_value(
    slot: EncoderSlot,
    lane: Lane,
    value: u8,
    pos: MidiTime,
    configs: &HashMap<EncoderSlot, EncoderConfig>,
    lfo_amounts: &HashMap<EncoderSlot, u8>,
    last_sent_output: &mut HashMap<EncoderSlot, u8>,
    modulators: &mut HashMap<EncoderSlot, Modulator>,
    lfo: &mut Lfo,
    scale: &Arc<Mutex<Scale>>,
    params: &Arc<Mutex<LoopGridParams>>,
    ui_feedback: UiFeedback,
) {
    if let Lane::LfoAmount(_) = lane {
        return;
    }

    let output_value = if let Some(config) = configs.get(&slot) {
        apply_lfo_to_value(
            value,
            lfo_amounts
                .get(&slot)
                .copied()
                .unwrap_or_else(|| neutral_lfo_amount(config)),
            pos,
            config,
            lfo,
        )
    } else {
        value
    };

    let should_send = last_sent_output
        .get(&slot)
        .copied()
        .map(|last| last != output_value)
        .unwrap_or(true);

    match modulators.get_mut(&slot).unwrap_or(&mut Modulator::None) {
        Modulator::None => {}
        Modulator::MidiModulator(instance) => {
            if should_send {
                instance.send(output_value);
                last_sent_output.insert(slot, output_value);
            }
        }
        Modulator::LfoSpeed(..) => {
            lfo.speed = value;
            if ui_feedback == UiFeedback::Manual {
                let mut params = params.lock().unwrap();
                params.lfo_speed_overlay_value = Some(lfo_speed_overlay_value(value));
                params.lfo_speed_overlay_until =
                    Some(Instant::now() + Duration::from_millis(ROOT_OVERLAY_MS));
            }
        }
        Modulator::LfoWave(..) => {
            lfo.wave = value;
            if ui_feedback == UiFeedback::Manual {
                let mut params = params.lock().unwrap();
                params.lfo_wave_overlay_mode = Some(lfo_wave_overlay_mode(value));
                params.lfo_wave_overlay_until =
                    Some(Instant::now() + Duration::from_millis(ROOT_OVERLAY_MS));
            }
        }
        Modulator::RootNote(..) => {
            let note = root_note_from_value(value);
            scale.lock().unwrap().root = note;
        }
        Modulator::SampleLevelMultiplier { sample, state } => {
            if should_send {
                let mut state = state.lock().unwrap();
                let index = *sample as usize;
                if index < state.multipliers.len() {
                    state.multipliers[index] = output_value;
                    state.dirty[index] = true;
                    last_sent_output.insert(slot, output_value);
                }
            }
        }
    }
}

fn apply_lfo_to_value(
    base_value: u8,
    lfo_amount: u8,
    pos: MidiTime,
    config: &EncoderConfig,
    lfo: &Lfo,
) -> u8 {
    if !config.assignment.supports_lfo_lane() || !lfo_amount_is_active(lfo_amount, config) {
        return base_value;
    }

    let phase = (lfo.get_value_at(pos) * 2.0) - 1.0;
    let depth = lfo_depth(lfo_amount, config);

    match config.lfo_mode {
        LfoMode::None => base_value,
        LfoMode::BipolarOffset => {
            let base = crate::controllers::midi_to_float(base_value);
            let amount = base + (phase * depth * 0.5);
            crate::controllers::float_to_midi(amount.max(0.0).min(1.0))
        }
        LfoMode::UnipolarMultiply => {
            let base = crate::controllers::midi_to_float(base_value);
            let uni = lfo.get_value_at(pos);
            let amount = base * (1.0 + (uni * depth));
            crate::controllers::float_to_midi(amount.max(0.0).min(1.0))
        }
    }
}

fn neutral_lfo_amount(config: &EncoderConfig) -> u8 {
    match config.lfo_mode {
        LfoMode::UnipolarMultiply => 0,
        _ => 64,
    }
}

fn lfo_amount_is_active(value: u8, config: &EncoderConfig) -> bool {
    value != neutral_lfo_amount(config)
}

fn lfo_depth(value: u8, config: &EncoderConfig) -> f64 {
    match config.lfo_mode {
        LfoMode::UnipolarMultiply => crate::controllers::midi_to_float(value),
        _ => crate::controllers::midi_to_polar(value).abs(),
    }
}

fn apply_encoder_delta(
    lane: Lane,
    delta: i8,
    configs: &HashMap<EncoderSlot, EncoderConfig>,
    base_values: &HashMap<EncoderSlot, u8>,
    lfo_amounts: &HashMap<EncoderSlot, u8>,
    prepared_values: &HashMap<Lane, u8>,
) -> u8 {
    let current = prepared_values
        .get(&lane)
        .copied()
        .unwrap_or_else(|| match lane {
            Lane::Base(slot) => base_values
                .get(&slot)
                .copied()
                .or_else(|| configs.get(&slot).map(|c| c.assignment.default_value()))
                .unwrap_or(0),
            Lane::LfoAmount(slot) => lfo_amounts
                .get(&slot)
                .copied()
                .or_else(|| configs.get(&slot).map(neutral_lfo_amount))
                .unwrap_or(64),
        });

    (current as i16 + delta as i16).clamp(0, 127) as u8
}

fn reset_lane(
    lane: Lane,
    configs: &HashMap<EncoderSlot, EncoderConfig>,
    base_values: &mut HashMap<EncoderSlot, u8>,
    lfo_amounts: &mut HashMap<EncoderSlot, u8>,
    loops: &mut HashMap<Lane, AutomationLoop>,
    last_sent_output: &mut HashMap<EncoderSlot, u8>,
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
                send_lane_value(
                    slot,
                    lane,
                    value,
                    MidiTime::zero(),
                    configs,
                    lfo_amounts,
                    last_sent_output,
                    modulators,
                    lfo,
                    scale,
                    params,
                    UiFeedback::Manual,
                );
            }
        }
        Lane::LfoAmount(slot) => {
            if let Some(config) = configs.get(&slot) {
                lfo_amounts.insert(slot, neutral_lfo_amount(config));
            }
        }
    }
}

fn refresh_feedback(
    configs: &HashMap<EncoderSlot, EncoderConfig>,
    base_values: &HashMap<EncoderSlot, u8>,
    lfo_amounts: &HashMap<EncoderSlot, u8>,
    prepared_values: &HashMap<Lane, u8>,
    loops: &HashMap<Lane, AutomationLoop>,
    recording_started: &HashMap<Lane, MidiTime>,
    params: &Arc<Mutex<LoopGridParams>>,
    shift: bool,
    current_bank: BankId,
    pulse: f64,
    pos: MidiTime,
    last_feedback: &mut HashMap<u8, SlotFeedback>,
    feedback: &mut midi_connection::SharedMidiOutputConnection,
) {
    let mut dirty_feedback: Vec<(u8, SlotFeedback)> = Vec::new();

    for (index, slot) in fixed_slots().iter().enumerate() {
        render_slot(
            *slot,
            index as u8,
            configs,
            base_values,
            lfo_amounts,
            prepared_values,
            loops,
            recording_started,
            params,
            shift,
            pulse,
            pos,
            last_feedback,
            &mut dirty_feedback,
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
            prepared_values,
            loops,
            recording_started,
            params,
            shift,
            pulse,
            pos,
            last_feedback,
            &mut dirty_feedback,
        );
    }

    send_encoder_feedback_frame(feedback, &dirty_feedback);
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
    prepared_values: &HashMap<Lane, u8>,
    loops: &HashMap<Lane, AutomationLoop>,
    recording_started: &HashMap<Lane, MidiTime>,
    params: &Arc<Mutex<LoopGridParams>>,
    shift: bool,
    pulse: f64,
    pos: MidiTime,
    last_feedback: &mut HashMap<u8, SlotFeedback>,
    dirty_feedback: &mut Vec<(u8, SlotFeedback)>,
) {
    let config = if let Some(config) = configs.get(&slot) {
        config
    } else {
        let new_feedback = SlotFeedback {
            switch_color: COLOR_OFF,
            switch_intensity: 0,
            ring_color: COLOR_OFF,
            ring_intensity: 0,
            ring_mode: EncoderRingType::default().to_midi(),
            ring_value: 0,
        };
        queue_slot_feedback(physical_index, new_feedback, last_feedback, dirty_feedback);
        return;
    };

    let active_lane = active_lane(slot, shift, configs);
    let is_recording = recording_started.contains_key(&active_lane);
    let has_loop = loops.contains_key(&active_lane);
    let has_prepared = prepared_values.contains_key(&active_lane);

    let activity_flash = has_activity_flash(&config.activity_highlights, params);
    let switch_color = if activity_flash {
        COLOR_WHITE
    } else {
        encoder_color_to_midi(config.color)
    };
    let switch_intensity = if activity_flash {
        ACTIVITY_FLASH_INTENSITY
    } else if has_prepared {
        AUTOMATION_BRIGHT_INTENSITY
    } else if has_loop {
        if pulse >= 0.5 {
            AUTOMATION_BRIGHT_INTENSITY
        } else {
            AUTOMATION_DIM_INTENSITY
        }
    } else {
        SWITCH_IDLE_INTENSITY
    };

    let lfo_amount = prepared_values
        .get(&Lane::LfoAmount(slot))
        .copied()
        .or_else(|| lfo_amounts.get(&slot).copied())
        .unwrap_or_else(|| neutral_lfo_amount(config));
    let has_lfo = config.assignment.supports_lfo_lane() && lfo_amount_is_active(lfo_amount, config);
    let lfo_color_phase = if has_lfo && !shift {
        let interval = MidiTime::from_ticks(LFO_ACTIVE_COLOR_SWAP_INTERVAL_TICKS);
        (pos % (interval * 2)).as_float() < interval.as_float()
    } else {
        false
    };

    let ring_value = if shift && config.assignment.supports_lfo_lane() {
        prepared_values
            .get(&Lane::LfoAmount(slot))
            .copied()
            .or_else(|| lfo_amounts.get(&slot).copied())
            .unwrap_or_else(|| neutral_lfo_amount(config))
    } else {
        prepared_values
            .get(&Lane::Base(slot))
            .copied()
            .or_else(|| base_values.get(&slot).copied())
            .unwrap_or_else(|| config.assignment.default_value())
    };

    let ring_type = if shift && config.assignment.supports_lfo_lane() {
        match config.lfo_mode {
            LfoMode::BipolarOffset => EncoderRingType::Pivot,
            _ => EncoderRingType::Fill,
        }
    } else {
        config.ring_type
    };

    let centered = is_centered_encoder(ring_type, ring_value);

    let ring_color = if is_recording {
        COLOR_RED
    } else if shift && config.assignment.supports_lfo_lane() {
        COLOR_PURPLE
    } else if lfo_color_phase {
        COLOR_PURPLE
    } else if centered {
        45
    } else {
        COLOR_WHITE
    };

    let ring_intensity = RING_IDLE_INTENSITY;

    let new_feedback = SlotFeedback {
        switch_color,
        switch_intensity,
        ring_color,
        ring_intensity,
        ring_mode: ring_type.to_midi(),
        ring_value,
    };

    queue_slot_feedback(physical_index, new_feedback, last_feedback, dirty_feedback);
}

fn queue_slot_feedback(
    physical_index: u8,
    new_feedback: SlotFeedback,
    last_feedback: &mut HashMap<u8, SlotFeedback>,
    dirty_feedback: &mut Vec<(u8, SlotFeedback)>,
) {
    if last_feedback
        .get(&physical_index)
        .copied()
        .map(|prev| prev == new_feedback)
        .unwrap_or(false)
    {
        return;
    }

    let _ = last_feedback.insert(physical_index, new_feedback);
    dirty_feedback.push((physical_index, new_feedback));
}

fn send_encoder_feedback_frame(
    feedback: &mut midi_connection::SharedMidiOutputConnection,
    records: &[(u8, SlotFeedback)],
) {
    if records.is_empty() {
        return;
    }

    let first_index = records.first().map(|(index, _)| *index).unwrap_or(0);
    let last_index = records.last().map(|(index, _)| *index).unwrap_or(0);

    let mut message =
        Vec::with_capacity(BULK_ENCODER_FEEDBACK_FRAME_PREFIX.len() + records.len() * 7 + 1);
    message.extend_from_slice(&BULK_ENCODER_FEEDBACK_FRAME_PREFIX);
    message[9] = records.len().min(127) as u8;

    for (physical_index, state) in records.iter().take(127) {
        message.push(*physical_index);
        message.push(state.ring_mode);
        message.push(state.ring_value);
        message.push(state.ring_color);
        message.push(state.ring_intensity);
        message.push(state.switch_color);
        message.push(state.switch_intensity);
    }

    message.push(0xF7);
    if let Err(err) = feedback.send(&message) {
        println!(
            "[WARN] Failed to queue encoder bulk frame: count={}, first_index={}, last_index={}, err={:?}",
            records.len(), first_index, last_index, err
        );
    }
}

fn is_centered_encoder(ring_type: EncoderRingType, value: u8) -> bool {
    ring_type == EncoderRingType::Pivot && value == 64
}

fn lfo_speed_overlay_value(value: u8) -> String {
    let labels = ["3B", "2B", "3/2", "1B", "1T", "2", "2T", "4", "4T", "8"];
    let index = (value as f64 * (labels.len() as f64 / 128.0)) as usize;
    labels[index.min(labels.len() - 1)].to_string()
}

fn lfo_wave_overlay_mode(value: u8) -> &'static str {
    let wave = value as f64 / 127.0;
    if wave < 0.2 {
        "TRI"
    } else if wave < 0.4 {
        "UP"
    } else if wave < 0.6 {
        "HUP"
    } else if wave < 0.8 {
        "HDN"
    } else {
        "DWN"
    }
}

fn root_note_from_value(value: u8) -> i32 {
    60 + (((value as i32 - 64) * 12) / 63)
}

fn has_activity_flash(
    highlights: &[ActivityHighlight],
    params: &Arc<Mutex<LoopGridParams>>,
) -> bool {
    if highlights.is_empty() {
        return false;
    }

    let now = Instant::now();
    let params = params.lock().unwrap();

    for highlight in highlights {
        match highlight {
            ActivityHighlight::Sample(index) => {
                if params
                    .activity_flash_until
                    .get(&sample_activity_channel(*index))
                    .map(|until| *until > now)
                    .unwrap_or(false)
                {
                    return true;
                }
            }
            ActivityHighlight::SampleRange(start, end) => {
                for index in *start..=*end {
                    if params
                        .activity_flash_until
                        .get(&sample_activity_channel(index))
                        .map(|until| *until > now)
                        .unwrap_or(false)
                    {
                        return true;
                    }
                }
            }
            ActivityHighlight::Samples => {
                for index in 0..8 {
                    if params
                        .activity_flash_until
                        .get(&sample_activity_channel(index))
                        .map(|until| *until > now)
                        .unwrap_or(false)
                    {
                        return true;
                    }
                }
            }
            ActivityHighlight::VoiceA => {
                if params
                    .activity_flash_until
                    .get(&5)
                    .map(|until| *until > now)
                    .unwrap_or(false)
                {
                    return true;
                }
            }
            ActivityHighlight::VoiceB => {
                if params
                    .activity_flash_until
                    .get(&4)
                    .map(|until| *until > now)
                    .unwrap_or(false)
                {
                    return true;
                }
            }
            ActivityHighlight::VoiceC => {
                if params
                    .activity_flash_until
                    .get(&6)
                    .map(|until| *until > now)
                    .unwrap_or(false)
                {
                    return true;
                }
            }
        }
    }

    false
}

fn sample_activity_channel(index: u8) -> u32 {
    match index {
        0 => 2,
        1 => 3,
        2 => 10,
        3 => 11,
        4 => 12,
        5 => 13,
        6 => 14,
        _ => 15,
    }
}

fn encoder_color_to_midi(color: EncoderColor) -> u8 {
    match color {
        EncoderColor::White => COLOR_WHITE,
        EncoderColor::Yellow => 19,
        EncoderColor::Orange => 10,
        EncoderColor::Blue => 75,
        EncoderColor::Purple => COLOR_PURPLE,
        EncoderColor::Pink => 122,
        EncoderColor::Cyan => 39,
        EncoderColor::Lime => 37,
        EncoderColor::Red => COLOR_RED,
        EncoderColor::Green => 43,
    }
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
