extern crate circular_queue;
extern crate midir;
use self::circular_queue::CircularQueue;
use std::time::{SystemTime, Duration};
use std::sync::mpsc;
use std::thread;
use std::collections::HashSet;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::sync::{Arc, Mutex};
use std::cmp::Ordering;

use ::midi_connection;
use ::midi_time::MidiTime;

use ::output_value::OutputValue;
use ::loop_recorder::{LoopRecorder, LoopEvent};
use ::loop_state::{LoopCollection, LoopState, LoopTransform, LoopStateChange};
use ::clock_source::{RemoteClock, ToClock, FromClock};
use ::chunk::{Triggerable, MidiMap, ChunkMap, Coords, LatchMode, ScheduleMode};
use ::scale::Scale;

const RIGHT_SIDE_BUTTONS: [u8; 8] = [89, 79, 69, 59, 49, 39, 29, 19];
const LEFT_SIDE_BUTTONS: [u8; 8] = [80, 70, 60, 50, 40, 30, 20, 10];
const TOP_BUTTONS: [u8; 8] = [91, 92, 93, 94, 95, 96, 97, 98];
const BOTTOM_BUTTONS: [u8; 4] = [1, 2, 3, 4];
const BANK_BUTTONS: [u8; 4] = [5, 6, 7, 8];
const BANK_COLORS: [u8; 4] = [15, 9, 59, 43];

const DEFAULT_VELOCITY: u8 = 100;

lazy_static! {
    static ref REPEAT_RATES: [MidiTime; 8] = [
        MidiTime::from_measure(2, 1),
        MidiTime::from_measure(1, 1),
        MidiTime::from_measure(2, 3),
        MidiTime::from_measure(1, 2),
        MidiTime::from_measure(1, 3),
        MidiTime::from_measure(1, 4),
        MidiTime::from_measure(1, 6),
        MidiTime::from_measure(1, 8)
    ];
    static ref LOOP_LENGTHS: [MidiTime; 8] = [
        MidiTime::from_beats(1),
        MidiTime::from_beats(2),
        MidiTime::from_beats(3),
        MidiTime::from_beats(4),
        MidiTime::from_beats(8),
        MidiTime::from_beats(16),
        MidiTime::from_beats(32),
        MidiTime::from_beats(64)
    ];
    static ref ALIGN_OFFSET_NUDGES: [MidiTime; 8] = [
        MidiTime::from_ticks(-12),
        MidiTime::from_ticks(-6),
        MidiTime::from_ticks(-3),
        MidiTime::from_ticks(-1),
        MidiTime::from_ticks(1),
        MidiTime::from_ticks(3),
        MidiTime::from_ticks(6),
        MidiTime::from_ticks(12)
    ];
}

pub struct LoopGridParams {
    pub swing: f64,
    pub bank: u8,
    pub frozen: bool,
    pub channel_repeat: HashMap<u32, ChannelRepeat>,
    pub align_offset: MidiTime,
    pub reset_automation: bool
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ChannelRepeat {
    None,
    Rate(MidiTime),
    Global
}

impl ChannelRepeat {
    pub fn from_midi (value: u8) -> ChannelRepeat {
        let pos = value / (127 / 9);
        if pos == 0 {
            ChannelRepeat::None
        } else if pos == 9 {
            ChannelRepeat::Global
        } else {
            ChannelRepeat::Rate(REPEAT_RATES[pos as usize - 1])
        }
    }

    pub fn to_midi (&self) -> u8 {
        match self {
            ChannelRepeat::None => 0,
            ChannelRepeat::Global => 127,
            ChannelRepeat::Rate(rate) => {
                if let Some(index) = REPEAT_RATES.iter().position(|x| x == rate) {
                    ((127 / 10) * (index + 1)) as u8
                } else {
                    0
                }
            }
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum LoopGridRemoteEvent {
    DoubleButton(bool),
    LoopButton(bool),
    SustainButton(bool)
}

#[derive(Debug, Copy, Clone)]
pub enum LoopGridMessage {
    Schedule(MidiTime, MidiTime),
    TwisterBank(u8),
    GridInput(u64, u32, OutputValue),
    LoopButton(bool),
    FlattenButton(bool),
    UndoButton(bool),
    RedoButton(bool),
    SuppressButton(bool),
    SelectButton(bool),
    HoldButton(bool),
    ScaleButton(bool),
    DoubleLoopLength,
    SustainButton(bool),
    Event(LoopEvent),
    InitialLoop,
    ClearRecording,
    RefreshInput(u32),
    RefreshOverride(u32),
    RefreshGridButton(u32),
    RefreshSelectionOverride,
    RefreshSelectedBank,
    RefreshSideButtons,
    RefreshLoopLength,
    RefreshShouldFlatten,
    RefreshActive,
    RefreshRecording,
    RefreshSelectState,
    RefreshLoopButton,
    ClearSelection,
    RefreshUndoRedoLights,
    SetRate(MidiTime),
    RateButton(usize, bool),
    LengthButton(usize, bool),
    TriggerChunk(MidiMap, OutputValue, SystemTime),
    TempoChanged(usize),
    RefreshSelectingScale,
    FlushChoke,
    ChunkTick,
    None
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
enum Light {
    // http://launchpaddr.com/mk2palette/
    Value(u8),
    Yellow,
    YellowMed,
    Lime,
    LimeLow,
    Purple,
    Green,
    GreenMed,
    GreenLow,
    GreenDark,
    Orange,
    OrangeMed,
    OrangeLow,
    Red,
    RedMed,
    RedLow,
    BlueDark,
    White,
    Off,
    None
}

impl Light {
    pub fn unwrap_or (self, value: Light) -> Light {
        match self {
            Light::None => value,
            _ => self
        }
    }

    pub fn value (&self) -> u8 {
        match self {
            Light::Yellow => 13,
            Light::YellowMed => 97,
            Light::Lime => 73,
            Light::LimeLow => 63,
            Light::Purple => 94,
            Light::Green => 17,
            Light::GreenMed => 76,
            Light::GreenLow => 18,
            Light::GreenDark => 35,
            Light::Orange => 96,
            Light::OrangeMed => 126,
            Light::OrangeLow => 105,
            Light::Red => 72,
            Light::RedMed => 120,
            Light::RedLow => 6,
            Light::BlueDark => 43,
            Light::White => 3,
            Light::Value(value) => *value,
            _ => 0
        }
    }
}

pub struct LoopGridLaunchpad {
    _input: midi_connection::ThreadReference,
    pub remote_tx: mpsc::Sender<LoopGridRemoteEvent>
}

impl LoopGridLaunchpad {
    pub fn new(launchpad_port_name: &str, chunk_map: Vec<Box<ChunkMap>>, scale: Arc<Mutex<Scale>>, params: Arc<Mutex<LoopGridParams>>, clock: RemoteClock) -> Self {
        let (tx, rx) = mpsc::channel();
        let (remote_tx, remote_rx) = mpsc::channel();

        let tx_remote =  mpsc::Sender::clone(&tx);
        let tx_input =  mpsc::Sender::clone(&tx);
        let tx_clock =  mpsc::Sender::clone(&tx);
        let tx_feedback =  mpsc::Sender::clone(&tx);
        let tx_loop_state =  mpsc::Sender::clone(&tx);

        let (midi_to_id, _id_to_midi) = get_grid_map();

        let mut launchpad_output = midi_connection::get_shared_output(&launchpad_port_name);
        launchpad_output.on_connect(move |port| {
            // send sysex message to put launchpad into live mode
            port.send(&[0xF0, 0x00, 0x20, 0x29, 0x02, 0x10, 0x40, 0x2F, 0x6D, 0x3E, 0x0A, 0xF7]).unwrap();
        });

        let input = midi_connection::get_input(&launchpad_port_name, move |stamp, message| {
            if message[0] == 144 || message[0] == 128 {
                let grid_button = midi_to_id.get(&message[1]);
                if grid_button.is_some() {
                    let value = if message[2] > 0 {
                        OutputValue::On(DEFAULT_VELOCITY)
                    } else {
                        OutputValue::Off
                    };

                    tx_input.send(LoopGridMessage::GridInput(stamp, *grid_button.unwrap(), value)).unwrap();
                } ;
            } else if message[0] == 176 {
                let active = message[2] > 0;

                if let Some(id) = LEFT_SIDE_BUTTONS.iter().position(|&x| x == message[1]) {
                    tx_input.send(LoopGridMessage::LengthButton(id, active)).unwrap();
                } else if let Some(id) = RIGHT_SIDE_BUTTONS.iter().position(|&x| x == message[1]) {
                    let active = message[2] > 0;
                    tx_input.send(LoopGridMessage::RateButton(id, active)).unwrap();
                } else if let Some(id) = TOP_BUTTONS.iter().position(|&x| x == message[1]) {
                    let to_send = match id {
                        0 => LoopGridMessage::LoopButton(active),
                        1 => LoopGridMessage::FlattenButton(active),
                        2 => LoopGridMessage::UndoButton(active),
                        3 => LoopGridMessage::RedoButton(active),
                        4 => LoopGridMessage::HoldButton(active),
                        5 => LoopGridMessage::SuppressButton(active),
                        6 => LoopGridMessage::ScaleButton(active),
                        7 => LoopGridMessage::SelectButton(active),
                        _ => LoopGridMessage::None
                    };
                    tx_input.send(to_send).unwrap();
                } else if let Some(id) = BANK_BUTTONS.iter().position(|&x| x == message[1]) {
                    // use last 4 bottom buttons as bank switchers
                    if message[2] > 0 {
                        tx_input.send(LoopGridMessage::TwisterBank(id as u8)).unwrap();
                    }
                } else if let Some(id) = BOTTOM_BUTTONS.iter().position(|&x| x == message[1]) {
                    let to_send = match id {
                        _ => LoopGridMessage::None
                    };
                    tx_input.send(to_send).unwrap();
                }
            }
        });

        let clock_sender = clock.sender.clone();

        // receive updates from clock
        thread::spawn(move || {
            for msg in clock.receiver {
                match msg {
                    FromClock::Schedule {pos, length} => {
                        tx_clock.send(LoopGridMessage::Schedule(pos, length)).unwrap();
                    },
                    FromClock::Tempo(value) => {
                        tx_clock.send(LoopGridMessage::TempoChanged(value)).unwrap();
                    },
                    FromClock::Jump => {
                        tx_clock.send(LoopGridMessage::InitialLoop).unwrap();
                    }
                }
            }
        });

        // receive messages from foot pedal
        thread::spawn(move || {
            for msg in remote_rx {
                match msg {
                    LoopGridRemoteEvent::LoopButton(pressed) => {
                        tx_remote.send(LoopGridMessage::LoopButton(pressed)).unwrap();
                    },
                    LoopGridRemoteEvent::DoubleButton(pressed) => {
                        if pressed {
                            tx_remote.send(LoopGridMessage::DoubleLoopLength).unwrap();
                        }
                    },
                    LoopGridRemoteEvent::SustainButton(pressed) => {
                        tx_remote.send(LoopGridMessage::SustainButton(pressed)).unwrap();
                    }
                }
            }
        });

        thread::spawn(move || {
            let mut mapping: HashMap<Coords, MidiMap> = HashMap::new();
            let mut chunks: Vec<Box<Triggerable>> = Vec::new();
            let mut chunk_latency_offsets: HashMap<usize, Duration> = HashMap::new();
            let mut chunk_colors: Vec<Light> = Vec::new();
            let mut chunk_channels: HashMap<usize, u32> = HashMap::new();
            let mut chunk_trigger_ids: Vec<Vec<u32>> = Vec::new();

            let mut no_suppress = HashSet::new();
            let mut trigger_latch_for: HashMap<usize, u32> = HashMap::new();
            let mut loop_length = MidiTime::from_beats(8);
            let mut base_loop = LoopCollection::new(loop_length);

            for mut item in chunk_map {
                let mut count = 0;
                let chunk_index = chunks.len();
                let mut trigger_ids = Vec::new();
                for row in (item.coords.row)..(item.coords.row + item.shape.rows) {
                    for col in (item.coords.col)..(item.coords.col + item.shape.cols) {
                        mapping.insert(Coords::new(row, col), MidiMap {chunk_index, id: count});   
                        trigger_ids.push(Coords::id_from(row, col));                
                        count += 1;
                    }
                }

                if item.chunk.latch_mode() == LatchMode::NoSuppress {
                    for id in &trigger_ids {
                        no_suppress.insert(*id);
                    }
                }

                if let Some(active) = item.chunk.get_active() {
                    for id in active {
                        if let Some(trigger_id) = trigger_ids.get(id as usize) {
                            if item.chunk.latch_mode() == LatchMode::LatchSingle {
                                trigger_latch_for.insert(chunk_index, *trigger_id);
                            } else {
                                base_loop.transforms.insert(*trigger_id, LoopTransform::Value(OutputValue::On(100)));
                            }
                        }
                    }
                }

                chunk_trigger_ids.push(trigger_ids);
                chunk_colors.push(Light::Value(item.color));

                if let Some(channel) = item.channel {
                    chunk_channels.insert(chunk_index, channel);
                }

                if let Some(latency_offset) = item.chunk.latency_offset() {
                    chunk_latency_offsets.insert(chunk_index, latency_offset);
                }

                chunks.push(item.chunk);
            }


            let mut loop_state = LoopState::new(loop_length, move |value, change| {
                loop_length = value.length;
                tx_loop_state.send(LoopGridMessage::InitialLoop).unwrap();
                tx_loop_state.send(LoopGridMessage::RefreshActive).unwrap();
                tx_loop_state.send(LoopGridMessage::RefreshLoopLength).unwrap();

                if change == LoopStateChange::Set {
                    tx_loop_state.send(LoopGridMessage::ClearRecording).unwrap();
                }
            });

            // create base level undo
            loop_state.set(base_loop);

            let mut repeating = false;
            let mut repeat_off_beat = false;
            let (_midi_to_id, id_to_midi) = get_grid_map();

            // selection
            let mut selection_override = LoopTransform::None;
            let mut selection: HashSet<u32> = HashSet::new();
            let mut suppressing = false;
            let mut holding = false;
            let mut holding_at = MidiTime::zero();
            let mut selecting = false;
            let mut selection_override_offset = None;
            let mut refresh_loop_length_in = None;

            let mut loop_held = false;
            let mut loop_from = MidiTime::from_ticks(0);
            let mut should_flatten = false;

            let mut selecting_scale = false;
            let mut selecting_scale_held = false;
            let mut last_selecting_scale = SystemTime::now();

            let mut rate = MidiTime::from_beats(2);
            let mut recorder = LoopRecorder::new();
    
            let mut last_tick_at = SystemTime::now();
            let mut last_tick_durations: CircularQueue<Duration> = CircularQueue::with_capacity(12);
            let mut tick_duration = Duration::from_millis(60 / 120 / 24 * 1000);

            let mut last_pos = MidiTime::from_ticks(0);
            let mut last_length = MidiTime::from_ticks(0);
            let mut align_offset = MidiTime::zero();

            let mut current_bank = 0;

            let mut sustained_values: HashMap<u32, LoopTransform> = HashMap::new();
            let mut override_values: HashMap<u32, LoopTransform> = HashMap::new();
            let mut input_values: HashMap<u32, OutputValue> = HashMap::new();
            let mut currently_held_inputs: Vec<u32> = Vec::new();
            let mut currently_held_rates: Vec<usize> = Vec::new();
            let mut last_changed_triggers: HashMap<u32, MidiTime> = HashMap::new();

            let mut frozen_loop: Option<LoopCollection> = None;

            // out state
            let mut current_swing: f64 = 0.0;
            let mut out_transforms: HashMap<u32, LoopTransform> = HashMap::new();
            let mut out_values: HashMap<u32, OutputValue> = HashMap::new();
            let mut grid_out: HashMap<u32, LaunchpadLight> = HashMap::new();
            let mut select_out = Light::Off;
            let mut last_repeat_light_out = Light::Off;
            let mut last_triggered: HashMap<usize, CircularQueue<u32>> = HashMap::new();

            let mut last_choke_output = HashMap::new();
            let mut choke_queue = HashSet::new();

            // display state
            let mut active: HashSet<u32> = HashSet::new();
            let mut recording: HashSet<u32> = HashSet::new();

            let mut last_beat_light = RIGHT_SIDE_BUTTONS[7];
            let mut last_repeat_light = RIGHT_SIDE_BUTTONS[7];

            // default button lights
            launchpad_output.send(&[176, TOP_BUTTONS[5], Light::RedLow.value()]).unwrap();
            launchpad_output.send(&[176, TOP_BUTTONS[6], Light::BlueDark.value()]).unwrap();
            tx_feedback.send(LoopGridMessage::RefreshLoopButton).unwrap();
            tx_feedback.send(LoopGridMessage::RefreshUndoRedoLights).unwrap();
            tx_feedback.send(LoopGridMessage::RefreshSelectedBank).unwrap();

            for id in 0..128 {
                tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
            }

            for received in rx {
                match received {
                    LoopGridMessage::Schedule(position, length) => {
                        let params = params.lock().unwrap();

                        // calculate bpm (unknown if syncing to external clock)
                        let current_time = SystemTime::now();
                        last_tick_durations.push(current_time.duration_since(last_tick_at).unwrap());
                        tick_duration = last_tick_durations.iter().sum::<Duration>() / (last_tick_durations.len() as u32);
                        last_tick_at = current_time;

                        // only read swing on 8th notes to prevent back scheduling
                        if (position - align_offset) % MidiTime::from_ticks(12) == MidiTime::zero() {
                            current_swing = params.swing;
                        }

                        // get the swung position
                        last_pos = (position - align_offset).swing(current_swing) + align_offset;
                        last_length = (position - align_offset + length).swing(current_swing) + align_offset - last_pos;

                        let mut events = get_events(last_pos, last_length, align_offset, &recorder, &out_transforms);

                        // peak at next tick events, so that we can trigger early if chunk needs latency compensation
                        for event in get_events(last_pos + last_length, last_length, align_offset, &recorder, &out_transforms) {
                            events.push(event);
                        }

                        let mut ranked = HashMap::new();
                        for (key, value) in &last_triggered {
                            for id in value.iter() {
                                *ranked.entry((key.clone(), id.clone())).or_insert(0) += 1;
                            }
                        }

                        // sort events so that earlier defined chunks schedule first
                        events.sort_by(|a, b| {
                            let a_mapping = mapping.get(&Coords::from(a.id));
                            let b_mapping = mapping.get(&Coords::from(b.id));
                            if let Some(a_mapping) = a_mapping {
                                if let Some(b_mapping) = b_mapping {
                                    let chunk_cmp = a_mapping.chunk_index.cmp(&b_mapping.chunk_index);
                                    let schedule_mode = chunks.get(a_mapping.chunk_index).unwrap().schedule_mode();
                                    return if chunk_cmp == Ordering::Equal && schedule_mode == ScheduleMode::Percussion {
                                        ranked.get(&(b_mapping.chunk_index, b_mapping.id)).unwrap_or(&0).cmp(ranked.get(&(a_mapping.chunk_index, a_mapping.id)).unwrap_or(&0))
                                    } else {
                                        chunk_cmp
                                    }
                                }
                            }
                            a.id.cmp(&b.id)
                        });

                        for event in events {
                            // apply event offset
                            if let Some(mapping) = mapping.get(&Coords::from(event.id)) {
                                let shifted_event_pos = if let Some(offset) = chunk_latency_offsets.get(&mapping.chunk_index) {
                                    pos_with_latency_compensation(tick_duration, event.pos, *offset)
                                } else {
                                    event.pos
                                };

                                // now filter out events that occur before or after the playback window
                                if shifted_event_pos >= last_pos && shifted_event_pos < (last_pos + last_length) {
                                    if event.value.is_on() {
                                        last_triggered.entry(mapping.chunk_index).or_insert(CircularQueue::with_capacity(8)).push(event.id);
                                    }

                                    tx_feedback.send(LoopGridMessage::Event(event)).unwrap();
                                }
                            }
                        }

                        if let Some(remain) = refresh_loop_length_in {
                            if remain > 0 { 
                                refresh_loop_length_in = Some(remain - 1);
                            } else {
                                refresh_loop_length_in = None;
                                tx_feedback.send(LoopGridMessage::RefreshLoopLength).unwrap();
                            }
                        }

                        if current_bank != params.bank {
                            current_bank = params.bank;
                            tx_feedback.send(LoopGridMessage::RefreshSelectedBank).unwrap();
                        }

                        tx_feedback.send(LoopGridMessage::ChunkTick).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshSideButtons).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshRecording).unwrap();
                    },
                    LoopGridMessage::TwisterBank(bank) => {
                        let mut params = params.lock().unwrap();
                        params.bank = bank;
                    },
                    LoopGridMessage::RefreshSelectedBank => {
                        for (index, id) in BANK_BUTTONS.iter().enumerate() {
                            if current_bank == index as u8 {
                                launchpad_output.send(&[176, *id as u8, Light::White.value()]).unwrap();
                            } else {
                                launchpad_output.send(&[176, *id as u8, BANK_COLORS[index]]).unwrap();
                            }
                        }
                    },
                    LoopGridMessage::RefreshSideButtons => {
                        let pos = last_pos - align_offset;

                        let beat_display_multiplier = (24.0 * 8.0) / loop_length.ticks() as f64;
                        let shifted_beat_position = (pos.ticks() as f64 * beat_display_multiplier / 24.0) as usize;

                        let current_beat_light = RIGHT_SIDE_BUTTONS[shifted_beat_position % 8];
                        let current_repeat_light = RIGHT_SIDE_BUTTONS[REPEAT_RATES.iter().position(|v| v == &rate).unwrap_or(0)];

                        let rate_color = if repeat_off_beat { Light::RedMed } else { Light::YellowMed };
                        let scale_color = Light::GreenDark;

                        if current_repeat_light != last_repeat_light || last_repeat_light_out != rate_color {
                            launchpad_output.send(&[144, last_repeat_light, 0]).unwrap();
                            launchpad_output.send(&[144, current_repeat_light, rate_color.value()]).unwrap();
                        }

                        let beat_start = pos.is_whole_beat();

                        let base_last_beat_light = if current_repeat_light == last_beat_light {
                            rate_color
                        } else {
                            Light::None
                        };
                        
                        let base_beat_light = if current_repeat_light == current_beat_light {
                            rate_color
                        } else {
                            Light::None
                        };
 
                        if current_beat_light != last_beat_light {
                            launchpad_output.send(&[144, last_beat_light, base_last_beat_light.unwrap_or(Light::Off).value()]).unwrap();
                            if !beat_start {
                                launchpad_output.send(&[144, current_beat_light, base_beat_light.unwrap_or(Light::GreenLow).value()]).unwrap();
                            }
                        }

                        if beat_start {
                            launchpad_output.send(&[144, current_beat_light, Light::White.value()]).unwrap();
                        } else if pos.beat_tick() == 3 {
                            launchpad_output.send(&[144, current_beat_light, base_beat_light.unwrap_or(Light::GreenLow).value()]).unwrap();
                        }

                        last_beat_light = current_beat_light;
                        last_repeat_light = current_repeat_light;
                        last_repeat_light_out = rate_color;
                    },
                    LoopGridMessage::RefreshUndoRedoLights => {
                        let color = if selecting_scale_held && selecting {
                            // nudging
                            Light::Orange
                        } else if selecting {
                            Light::GreenLow
                        } else {
                            Light::RedLow
                        };

                        launchpad_output.send(&[176, TOP_BUTTONS[2], color.value()]).unwrap();
                        launchpad_output.send(&[176, TOP_BUTTONS[3], color.value()]).unwrap();
                    },
                    LoopGridMessage::RefreshLoopButton => {
                        launchpad_output.send(&[176, TOP_BUTTONS[0], Light::YellowMed.value()]).unwrap();
                    },
                    LoopGridMessage::RefreshLoopLength => {
                        for (index, id) in LEFT_SIDE_BUTTONS.iter().enumerate() {

                            let prev_button_length = *LOOP_LENGTHS.get(index.wrapping_sub(1)).unwrap_or(&MidiTime::zero());
                            let button_length = LOOP_LENGTHS[index];
                            let next_button_length = *LOOP_LENGTHS.get(index + 1).unwrap_or(&(LOOP_LENGTHS[LOOP_LENGTHS.len() - 1] * 2));

                            let result = if button_length == loop_length {
                                Light::Yellow
                            } else if loop_length < button_length && loop_length > prev_button_length {
                                Light::Red
                            } else if loop_length > button_length && loop_length < next_button_length {
                                Light::Red
                            } else {
                                Light::Off
                            };

                            launchpad_output.send(&[176, *id, result.value()]).unwrap();
                        }
                    },
                    LoopGridMessage::GridInput(_stamp, id, value) => {
                        let current_index = currently_held_inputs.iter().position(|v| v == &id);
                        let scale_id = id + 64;
                        
                        if value.is_on() && current_index == None {
                            currently_held_inputs.push(id);
                        } else if let Some(index) = current_index {
                            currently_held_inputs.remove(index);
                        }

                        if selecting && value.is_on() {
                            if selection.contains(&id) {
                                selection.remove(&scale_id);
                                selection.remove(&id);
                            } else {
                                if selecting_scale { // hack to avoid including drums/vox
                                    selection.insert(scale_id);
                                } else {
                                    selection.insert(id);
                                }
                            }

                            // range selection
                            if currently_held_inputs.len() == 2 {
                                let from = Coords::from(currently_held_inputs[0]);
                                let to = Coords::from(currently_held_inputs[1]);

                                let from_row = u32::min(from.row, to.row);
                                let to_row = u32::max(from.row, to.row) + 1;
                                let from_col = u32::min(from.col, to.col);
                                let to_col = u32::max(from.col, to.col) + 1;

                                for row in from_row..to_row {
                                    for col in from_col..to_col {
                                        let row_offset = if selecting_scale { 8 } else { 0 };
                                        let id = Coords::id_from(row + row_offset, col);
                                        selection.insert(id);
                                        tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
                                    }
                                }
                            }

                            tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
                        } else {
                            let in_scale_view = (selecting_scale && (selection.len() == 0 || !selection.contains(&id))) || selection.contains(&scale_id);

                            if in_scale_view  {
                                input_values.insert(scale_id, value);
                                input_values.remove(&id);
                            } else {
                                input_values.insert(id, value);
                                input_values.remove(&(scale_id));
                            }
                            tx_feedback.send(LoopGridMessage::RefreshInput(id)).unwrap();
                            tx_feedback.send(LoopGridMessage::RefreshInput(scale_id)).unwrap();
                        }
                        tx_feedback.send(LoopGridMessage::RefreshShouldFlatten).unwrap();
                    },
                    LoopGridMessage::RefreshInput(id) => {
                        let value = input_values.get(&id).unwrap_or(&OutputValue::Off);
                        let transform = match value {
                            &OutputValue::On(velocity) => {
                                if let Some(mapped) = mapping.get(&Coords::from(id)) {
                                    match get_repeat_for(mapped.chunk_index, &chunk_channels, &params) {
                                        ChannelRepeat::None => LoopTransform::Value(OutputValue::On(velocity)),
                                        ChannelRepeat::Rate(rate) => LoopTransform::Repeat {rate, offset: MidiTime::zero(), value: OutputValue::On(velocity)},
                                        ChannelRepeat::Global => {
                                            if repeating {
                                                let offset = if repeat_off_beat { rate / 2 } else { MidiTime::zero() };
                                                LoopTransform::Repeat {rate, offset, value: OutputValue::On(velocity)}
                                            } else {
                                                LoopTransform::Value(OutputValue::On(velocity))
                                            }
                                        }
                                    }
                                } else {
                                    LoopTransform::None
                                }
                            },
                            &OutputValue::Off => LoopTransform::None
                        };

                        let changed = match override_values.entry(id) {
                            Occupied(mut entry) => {
                                let different = entry.get() != &transform;
                                entry.insert(transform);
                                different
                            },
                            Vacant(entry) => {
                                let different = transform != LoopTransform::None;
                                entry.insert(transform);
                                different
                            }
                        };

                        if changed {
                            if get_schedule_mode(id, &chunks, &mapping) == ScheduleMode::Monophonic {
                                // refresh all in this chunk if monophonic
                                for id in get_all_ids_in_this_chunk(id, &chunks, &mapping, &chunk_trigger_ids) {
                                    tx_feedback.send(LoopGridMessage::RefreshOverride(id)).unwrap();
                                }
                            } else if selection.contains(&id) {
                                // refresh all in selection if part of selection
                                for id in &selection {
                                    tx_feedback.send(LoopGridMessage::RefreshOverride(*id)).unwrap();
                                }
                            } else {
                                tx_feedback.send(LoopGridMessage::RefreshOverride(id)).unwrap();
                            }
                        }
                    },
                    LoopGridMessage::RefreshOverride(id) => {
                        // use frozen loop if present
                        let loop_collection = if let Some(frozen_loop) = &frozen_loop {
                            frozen_loop
                        } else {
                            loop_state.get()
                        };

                        let selection_override_loop_collection = if frozen_loop.is_some() {
                            None
                        } else if let Some(offset) = selection_override_offset {
                            loop_state.retrieve(offset)
                        } else {
                            None
                        };

                        let mut transform = get_transform(id, &sustained_values, &override_values, &selection, &selection_override, &loop_collection, selection_override_loop_collection, &no_suppress);

                        // suppress if there are inputs held and monophonic scheduling
                        if get_schedule_mode(id, &chunks, &mapping) == ScheduleMode::Monophonic && transform.is_active() {
                            if !override_values.get(&id).unwrap_or(&LoopTransform::None).is_active() {
                                // now check to see if any other triggers in the chunk have overrides
                                let ids = get_all_ids_in_this_chunk(id, &chunks, &mapping, &chunk_trigger_ids);
                                let chunk_has_override = ids.iter().any(|id| override_values.get(id).unwrap_or(&LoopTransform::None).is_active());
                                if chunk_has_override {
                                    // suppress this override
                                    transform = LoopTransform::Value(OutputValue::Off);
                                }
                            }
                        }

                        // if this note is part of selection, and other notes in selection are being overridden, then suppress this trigger
                        let selection_active = selection.iter().any(|x| override_values.get(x).unwrap_or(&LoopTransform::None).is_active());
                        if transform.is_active() && !override_values.get(&id).unwrap_or(&LoopTransform::None).is_active() && selection.contains(&id) && selection_active {
                            transform = LoopTransform::Value(OutputValue::Off);
                        }

                        if out_transforms.get(&id).unwrap_or(&LoopTransform::None).unwrap_or(&LoopTransform::Value(OutputValue::Off)) != transform.unwrap_or(&LoopTransform::Value(OutputValue::Off)) {
                            out_transforms.insert(id, transform);

                            last_changed_triggers.insert(id, last_pos);

                            let pos = current_pos(last_pos, last_tick_at, tick_duration);
 
                            // send new value
                            if let Some(value) = get_value(id, pos, align_offset, &recorder, &out_transforms, true) {
                                tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                                    id, value, pos
                                })).unwrap();
                            }
                        }
                    },  
                    LoopGridMessage::TempoChanged(_value) => {
                        // text is not supported by launchpad pro in Live mode
                        // launchpad_output.send(&launchpad_text(&value.to_string())).unwrap();
                    },
                    LoopGridMessage::RefreshGridButton(id) => {
                        let base_id = id % 64;

                        let in_scale_view = (selecting_scale && (selection.len() == 0 || !selection.contains(&id))) || 
                            (selecting && selecting_scale_held) || 
                            selection.contains(&(base_id + 64));

                        let (id, background_id) = if in_scale_view {
                            (base_id + 64, base_id)
                        } else {
                            (base_id, base_id + 64)
                        };

                        let mapped = mapping.get(&Coords::from(id));
                        let background_mapped = mapping.get(&Coords::from(background_id));

                        let triggering = if out_values.get(&id).unwrap_or(&OutputValue::Off).is_on() {
                            true
                        } else if mapped.is_some() && trigger_latch_for.contains_key(&mapped.unwrap().chunk_index) {
                            trigger_latch_for.get(&mapped.unwrap().chunk_index).unwrap() == &id
                        } else {
                            false
                        };

                        let background_triggering = if out_values.get(&background_id).unwrap_or(&OutputValue::Off).is_on() {
                            true
                        } else if background_mapped.is_some() && trigger_latch_for.contains_key(&background_mapped.unwrap().chunk_index) {
                            trigger_latch_for.get(&background_mapped.unwrap().chunk_index).unwrap() == &background_id
                        } else {
                            false
                        };

                        let old_value = grid_out.remove(&base_id).unwrap_or(LaunchpadLight::Constant(Light::Off));

                        let color = if let Some(mapped) = mapped {
                            chunk_colors[mapped.chunk_index]
                        } else {
                            Light::Off
                        };

                        let selection_color = if in_scale_view {
                            Light::Purple
                        } else {
                            Light::Green
                        };

                        let background_color = if let Some(background_mapped) = background_mapped {
                            chunk_colors[background_mapped.chunk_index]
                        } else {
                            Light::Off
                        };

                        let new_value = if triggering && selection.contains(&id) {
                            LaunchpadLight::Pulsing(Light::White)
                        } else if triggering {
                            LaunchpadLight::Constant(Light::White)
                        } else if selection.contains(&id) {
                            LaunchpadLight::Pulsing(selection_color)
                        } else if recording.contains(&id) {
                            LaunchpadLight::Pulsing(Light::RedLow)
                        } else if background_triggering {
                            LaunchpadLight::Constant(background_color)
                        } else if active.contains(&id) {
                            LaunchpadLight::Pulsing(color)
                        } else {
                            LaunchpadLight::Constant(color)
                        };

                        if new_value != old_value {
                            let midi_id = id_to_midi.get(&base_id);
                            let message = match new_value {
                                LaunchpadLight::Constant(value) => [144, *midi_id.unwrap(), value.value()],
                                LaunchpadLight::Pulsing(value) => [146, *midi_id.unwrap(), value.value()]
                            };
                            launchpad_output.send(&message).unwrap()
                        }

                        grid_out.insert(base_id, new_value);
                    },
                    LoopGridMessage::RefreshSelectionOverride => {
                        selection_override = if suppressing {
                            LoopTransform::Value(OutputValue::Off)
                        } else if holding {
                            LoopTransform::Range {pos: holding_at, length: rate}
                        } else {
                            LoopTransform::None
                        };

                        for id in 0..128 {
                            tx_feedback.send(LoopGridMessage::RefreshOverride(id)).unwrap();
                        }
                    },
                    LoopGridMessage::RefreshActive => {
                        let current_loop = loop_state.get();
                        let selection_override_loop_collection = if frozen_loop.is_some() {
                            None
                        } else if let Some(offset) = selection_override_offset {
                            loop_state.retrieve(offset)
                        } else {
                            None
                        };

                        let mut ids = HashSet::new();
                        for (id, transform) in &current_loop.transforms {
                            if is_active(transform, id, &recorder) {
                                ids.insert(*id);
                            }
                        }

                        for id in &selection {
                            if let Some(override_loop) = selection_override_loop_collection {
                                if is_active(override_loop.transforms.get(id).unwrap_or(&LoopTransform::None), id, &recorder) {
                                    ids.insert(*id);
                                } else {
                                    ids.remove(id);
                                }
                            }
                        }

                        let (added, removed) = update_ids(&ids, &mut active);
 
                        for id in added {
                            tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
                        }

                        for id in removed {
                            tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
                        }
                    },
                    LoopGridMessage::RefreshRecording => {
                        let mut ids = HashSet::new();

                        let from = if loop_held {
                            loop_from
                        } else {
                            last_pos - loop_length
                        }; 

                        for (id, last_changed) in &last_changed_triggers {
                            if last_changed >= &from {
                                ids.insert(*id);
                            }
                        }

                        // for (id, value) in &override_values  {
                        //     if value != &LoopTransform::None {
                        //         ids.insert(*id);
                        //     }
                        // }

                        let (added, removed) = update_ids(&ids, &mut recording);

                        for id in added {
                            tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
                        }

                        for id in removed {
                            tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
                        }
                    },
                    LoopGridMessage::RefreshSelectState => {
                        let new_state = if selecting {
                            Light::Green
                        } else if selection.len() > 0 {
                            Light::GreenLow
                        } else {
                            Light::Off
                        };

                        if select_out != new_state {
                            launchpad_output.send(&[178, TOP_BUTTONS[7], new_state.value()]).unwrap();
                            select_out = new_state;
                        }
                    },
                    LoopGridMessage::Event(event) => {
                        if let Some(mapped) = mapping.get(&Coords::from(event.id)) {
                            let new_value = event.value.clone();
                            let offset = event.pos - last_pos;
                            println!("offset {}", offset.as_float());
                            let tick_nano = (tick_duration.subsec_nanos() as f64 * offset.as_float()) as u32;
                            
                            let latency_offset = chunk_latency_offsets.get(&mapped.chunk_index).unwrap_or(&Duration::from_nanos(0));

                            let time = last_tick_at + Duration::new(0, tick_nano) - *latency_offset;
                            
                            match maybe_update(&mut out_values, event.id, new_value) {
                                Some(_) => {
                                    if let Some(chunk) = chunks.get(mapped.chunk_index) {
                                        if chunk.latch_mode() == LatchMode::LatchSingle && new_value.is_on() {
                                            // track last triggered
                                            if let Some(id) = trigger_latch_for.get(&mapped.chunk_index) {
                                                // queue refresh of previous trigger latch
                                                tx_feedback.send(LoopGridMessage::RefreshGridButton(id.clone())).unwrap();
                                            }
                                            trigger_latch_for.insert(mapped.chunk_index, event.id);
                                        }
                                    }
                                    
                                    tx_feedback.send(LoopGridMessage::RefreshGridButton(event.id)).unwrap();
                                    tx_feedback.send(LoopGridMessage::TriggerChunk(*mapped, new_value, time)).unwrap();
                                },
                                None => ()
                            };
                        }

                        recorder.add(event);
                    },
                    LoopGridMessage::ClearRecording => {
                        last_changed_triggers.clear();
                    },
                    LoopGridMessage::LoopButton(pressed) => {
                        if selecting_scale && selecting {
                            if pressed {
                                clock_sender.send(ToClock::TapTempo).unwrap();
                            }
                        } else {
                            if pressed {
                                commit_selection_override(&mut selection_override_offset, &mut loop_state, &selection, &mut last_changed_triggers, &last_pos);
                                loop_held = true;
                                loop_from = last_pos;
                                launchpad_output.send(&[176, TOP_BUTTONS[0], Light::Green.value()]).unwrap();
                            } else {
                                loop_held = false;
                                tx_feedback.send(LoopGridMessage::RefreshLoopButton).unwrap();
                                let since_press = last_pos - loop_from;
                                let threshold = MidiTime::from_ticks(20);
                                let mut new_loop = loop_state.get().clone();

                                if since_press > threshold {
                                    // loop range between loop button down and up
                                    let quantized_length = MidiTime::quantize_length(last_pos - loop_from);
                                    loop_length = quantized_length;
                                    tx_feedback.send(LoopGridMessage::RefreshLoopLength).unwrap();
                                } else {
                                    // loop range to loop button down using last loop_length
                                    loop_from = loop_from - loop_length
                                }

                                let mut recording_ids = HashSet::new();

                                for (id, last_change) in &last_changed_triggers {
                                    if last_change > &loop_from {
                                        recording_ids.insert(*id);
                                    }
                                }

                                for (id, value) in &override_values {
                                    if value != &LoopTransform::None {
                                        recording_ids.insert(*id);
                                    }
                                }

                                for id in &selection {
                                    // include events in selection when looping 
                                    recording_ids.insert(*id);
                                }

                                for id in 0..128 {
                                    // include ids that are recording, or if selecting, all active IDs!
                                    let selected = selecting || selection.contains(&id);
                                    if recording_ids.contains(&id) || (selected && active.contains(&id)) {

                                        // only include in loop if there are items in the range
                                        let current_event = recorder.get_event_at(id, loop_from);
                                        let has_events = recorder.has_events(id, loop_from, loop_from + loop_length);
                                        if has_events || current_event.is_some() {
                                            new_loop.transforms.insert(id, LoopTransform::Range {
                                                pos: loop_from, 
                                                length: loop_length
                                            });
                                        } else {
                                            new_loop.transforms.insert(id, LoopTransform::None);
                                        }
                                    }
                                }

                                if new_loop.transforms.len() > 0 {
                                    new_loop.length = loop_length;
                                    loop_state.set(new_loop);
                                    tx_feedback.send(LoopGridMessage::ClearRecording).unwrap();
                                }

                                tx_feedback.send(LoopGridMessage::ClearSelection).unwrap();
                            }
                        }
                    },
                    LoopGridMessage::ClearSelection => {
                        commit_selection_override(&mut selection_override_offset, &mut loop_state, &selection, &mut last_changed_triggers, &last_pos);

                        for id in &selection {
                            tx_feedback.send(LoopGridMessage::RefreshGridButton(*id)).unwrap();
                        }

                        if !selecting_scale {
                            selecting_scale = false;
                            tx_feedback.send(LoopGridMessage::RefreshSelectingScale).unwrap();
                        }

                        selection.clear();

                        tx_feedback.send(LoopGridMessage::RefreshSelectState).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshSelectionOverride).unwrap();
                    },
                    LoopGridMessage::RefreshShouldFlatten => {
                        let new_value = &selection_override != &LoopTransform::None || override_values.values().any(|value| value != &LoopTransform::None) || sustained_values.len() > 0;
                        if new_value != should_flatten {
                            should_flatten = new_value;
                            let color = if should_flatten {
                                Light::GreenLow
                            } else {
                                Light::Off
                            };
                            launchpad_output.send(&[176, TOP_BUTTONS[1], color.value()]).unwrap();
                        }
                    },
                    LoopGridMessage::FlattenButton(pressed) => {
                        if pressed {
                            commit_selection_override(&mut selection_override_offset, &mut loop_state, &selection, &mut last_changed_triggers, &last_pos);

                            if should_flatten {
                                let mut new_loop = loop_state.get().clone();

                                for id in 0..128 {
                                    let mut transform = out_transforms.get(&id).unwrap_or(&LoopTransform::None).clone();
                                   
                                    // check if there are actually events available for this range
                                    let is_empty = if let LoopTransform::Range {pos, length} = transform {
                                        !recorder.has_events(id, pos, pos + length)
                                    } else {
                                        false
                                    };

                                    if is_empty {
                                        new_loop.transforms.insert(id.clone(), LoopTransform::None);
                                    } else {
                                        new_loop.transforms.insert(id.clone(), transform);
                                    }
                                }

                                loop_state.set(new_loop);
                            } else if selection.len() > 0 {
                                let mut new_loop = loop_state.get().clone();

                                for id in &selection {
                                    new_loop.transforms.insert(id.clone(), LoopTransform::Value(OutputValue::Off));
                                }

                                loop_state.set(new_loop);
                            } else {
                                let mut new_loop = loop_state.get().clone();

                                if selecting {
                                    for id in 0..128 {
                                        if !no_suppress.contains(&id) {
                                            new_loop.transforms.insert(id, LoopTransform::Value(OutputValue::Off));
                                        }
                                    }

                                    // HACK: send a message to twister to clear automation
                                    let mut params = params.lock().unwrap();
                                    params.reset_automation = true;
                                } else {
                                    if selecting_scale {
                                        for id in 64..128 {
                                            // just erase scale events
                                            if !no_suppress.contains(&id) {
                                                new_loop.transforms.insert(id, LoopTransform::Value(OutputValue::Off));
                                            }
                                        }
                                    } else {
                                        for id in 0..64 {
                                            // just erase scale events
                                            if !no_suppress.contains(&id) {
                                                new_loop.transforms.insert(id, LoopTransform::Value(OutputValue::Off));
                                            }
                                        }
                                    }
                                }
                                

                                loop_state.set(new_loop);
                            }
                            tx_feedback.send(LoopGridMessage::ClearSelection).unwrap();
                        }
                    },
                    LoopGridMessage::UndoButton(pressed) => {
                        if pressed {
                            if selecting && selecting_scale_held {
                                // nudge clock backwards (modify timing of existing loop)
                                clock_sender.send(ToClock::Nudge(MidiTime::from_ticks(-1))).unwrap();
                            } else if selecting {
                                loop_length = get_half_loop_length(loop_length).max(MidiTime::from_measure(1, 4));
                                tx_feedback.send(LoopGridMessage::RefreshLoopLength).unwrap();
                            } else if selection.len() > 0 {
                                if let Some(next_offset) = loop_state.previous_index_for(selection_override_offset.unwrap_or(0), &selection) {
                                    selection_override_offset = Some(next_offset);
                                    tx_feedback.send(LoopGridMessage::RefreshSelectionOverride).unwrap();
                                }
                            } else {
                                loop_state.undo();
                            }
                        }
                    },
                    LoopGridMessage::DoubleLoopLength => {
                        loop_length = get_double_loop_length(loop_length).min(MidiTime::from_beats(32));
                        tx_feedback.send(LoopGridMessage::RefreshLoopLength).unwrap();
                    },
                    LoopGridMessage::RedoButton(pressed) => {
                        if pressed {
                            if selecting && selecting_scale_held {
                                // nudge clock forwards
                                clock_sender.send(ToClock::Nudge(MidiTime::from_ticks(1))).unwrap();
                            } else if selecting { 
                                tx_feedback.send(LoopGridMessage::DoubleLoopLength).unwrap();
                            } else if selection.len() > 0 {
                                if let Some(next_offset) = loop_state.next_index_for(selection_override_offset.unwrap_or(0), &selection) {
                                    selection_override_offset = Some(next_offset);
                                    tx_feedback.send(LoopGridMessage::RefreshSelectionOverride).unwrap();
                                }
                            } else {
                                loop_state.redo();
                            }
                        }
                    },  
                    LoopGridMessage::HoldButton(pressed) => {
                        holding = pressed;
                        holding_at = last_pos;
                        tx_feedback.send(LoopGridMessage::RefreshSelectionOverride).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshShouldFlatten).unwrap();
                    },
                    LoopGridMessage::SuppressButton(pressed) => {
                        suppressing = pressed;
                        tx_feedback.send(LoopGridMessage::RefreshSelectionOverride).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshShouldFlatten).unwrap();
                    },
                    LoopGridMessage::SelectButton(pressed) => {
                        selecting = pressed;
                        if pressed {
                            tx_feedback.send(LoopGridMessage::ClearSelection).unwrap()
                        }

                        tx_feedback.send(LoopGridMessage::RefreshSelectingScale).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshSelectState).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshUndoRedoLights).unwrap();
                    },
                    LoopGridMessage::ScaleButton(pressed) => {
                        if pressed {
                            last_selecting_scale = SystemTime::now();
                            selecting_scale = !selecting_scale;
                        } else if last_selecting_scale.elapsed().unwrap() > Duration::from_millis(300) {
                            selecting_scale = !selecting_scale;
                        }

                        selecting_scale_held = pressed;

                        tx_feedback.send(LoopGridMessage::RefreshSelectingScale).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshUndoRedoLights).unwrap();
                    },
                    LoopGridMessage::SustainButton(pressed) => {
                        // send frozen to twister
                        let mut params = params.lock().unwrap();
                        params.frozen = pressed;

                        if pressed {
                            let current_loop = loop_state.get();
                            frozen_loop = Some(current_loop.clone());
        
                            for (id, value) in &override_values {
                                if value != &LoopTransform::None {
                                    sustained_values.insert(*id, value.clone());
                                }
                            }
                        } else {
                            frozen_loop = None;
                            sustained_values.clear();
                        }

                        for id in 0..128 {
                            tx_feedback.send(LoopGridMessage::RefreshOverride(id)).unwrap();
                        }

                        tx_feedback.send(LoopGridMessage::RefreshShouldFlatten).unwrap();
                    },
                    LoopGridMessage::RefreshSelectingScale => {
                        if selecting_scale {
                            launchpad_output.send(&[178, TOP_BUTTONS[6], Light::Yellow.value()]).unwrap();    
                        } else {
                            launchpad_output.send(&[176, TOP_BUTTONS[6], Light::BlueDark.value()]).unwrap();
                        };


                        for id in 0..64 {
                            tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
                        }
                    }
                    LoopGridMessage::RateButton(button_id, pressed) => {
                        let current_index = currently_held_rates.iter().position(|v| v == &button_id);

                        if pressed && current_index == None {
                            currently_held_rates.push(button_id);
                        } else if let Some(index) = current_index {
                            currently_held_rates.remove(index);
                        }

                        if currently_held_rates.len() > 0 {
                            let id = *currently_held_rates.iter().last().unwrap();
                            let rate = REPEAT_RATES[id];
                            tx_feedback.send(LoopGridMessage::SetRate(rate)).unwrap();
                            repeat_off_beat = selecting;
                            repeating = id > 0 || repeat_off_beat;
                        }
                    },
                    LoopGridMessage::LengthButton(button_id, pressed) => {
                        if pressed {
                            if selecting {
                                // nudge align offset
                                let nudge_offset = ALIGN_OFFSET_NUDGES[button_id % ALIGN_OFFSET_NUDGES.len()];
                                align_offset = align_offset + nudge_offset;

                                // flash offset amount
                                let iter = if button_id < 4 {
                                    button_id..4
                                } else {
                                    4..(button_id + 1)
                                };

                                for id in iter {
                                    launchpad_output.send(&[176, LEFT_SIDE_BUTTONS[id], Light::Purple.value()]).unwrap();
                                }

                                refresh_loop_length_in = Some(nudge_offset.ticks().abs());
    
                            } else {
                                loop_length = LOOP_LENGTHS[button_id % LOOP_LENGTHS.len()];
                                tx_feedback.send(LoopGridMessage::RefreshLoopLength).unwrap();
                            }
                        }
                    },
                    LoopGridMessage::SetRate(value) => {
                        rate = value;
                        tx_feedback.send(LoopGridMessage::RefreshSideButtons).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshSelectionOverride).unwrap();

                        let mut to_update = HashMap::new();
                        for (id, value) in &override_values {
                            if let Some(mapped) = mapping.get(&Coords::from(*id)) {
                                if get_repeat_for(mapped.chunk_index, &chunk_channels, &params) == ChannelRepeat::Global {
                                    if let &LoopTransform::Repeat {rate: _, offset, value} = value {
                                        to_update.insert(*id, LoopTransform::Repeat {rate, offset, value});
                                    }
                                }
                            }
                        }
                        for (id, value) in to_update {
                            override_values.insert(id, value);
                            tx_feedback.send(LoopGridMessage::RefreshOverride(id)).unwrap();
                        }
                        
                    },                 
                    LoopGridMessage::InitialLoop => {
                        let params = params.lock().unwrap();

                        for id in 0..128 {
                            let loop_collection = if let Some(frozen_loop) = &frozen_loop {
                                frozen_loop
                            } else {
                                loop_state.get()
                            };

                            let selection_override_loop_collection = if let Some(offset) = selection_override_offset {
                                loop_state.retrieve(offset)
                            } else {
                                None
                            };
                            let transform = get_transform(id, &sustained_values, &override_values, &selection, &selection_override, &loop_collection, selection_override_loop_collection, &no_suppress);
                            
                            if out_transforms.get(&id).unwrap_or(&LoopTransform::None) != &transform {
                                out_transforms.insert(id, transform);
                                last_changed_triggers.insert(id, last_pos);

                                // send new value
                                if let Some(value) = get_value(id, last_pos, params.align_offset, &recorder, &out_transforms, false) {
                                    tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                                        id: id, value, pos: last_pos
                                    })).unwrap();
                                }
                            }
                        }
                    },
                    LoopGridMessage::TriggerChunk(map, value, time) => {
                        if let Some(chunk) = chunks.get_mut(map.chunk_index) {
                            if let Some(chokes) = chunk.get_chokes_for(map.id) {
                                match value {
                                    OutputValue::Off => {},
                                    OutputValue::On(_) => {
                                        // queue up choke for next cycle (fake event loop)
                                        for id in chokes {
                                            choke_queue.insert((map.chunk_index, id));
                                        }

                                        tx_feedback.send(LoopGridMessage::FlushChoke).unwrap();

                                        last_choke_output.insert(map.chunk_index, map.id);
                                        choke_queue.remove(&(map.chunk_index, map.id));
                                        chunk.trigger(map.id, value, time);
                                    }
                                }
                            } else {
                                chunk.trigger(map.id, value, time);
                            }
                        }
                    },
                    LoopGridMessage::ChunkTick => {
                        for chunk in &mut chunks {
                            chunk.on_tick(last_pos);
                        }
                    },
                    LoopGridMessage::FlushChoke => {
                        for &(chunk_index, id) in &choke_queue {
                            if let Some(chunk) = chunks.get_mut(chunk_index) {
                                chunk.trigger(id, OutputValue::Off, SystemTime::now());
                            }
                        }
                        choke_queue.clear();
                    },
                    LoopGridMessage::None => ()
                }
            }
        });

        LoopGridLaunchpad {
            _input: input,
            remote_tx
        }
    }
}

fn maybe_update (hash_map: &mut HashMap<u32, OutputValue>, key: u32, new_value: OutputValue) -> Option<OutputValue> {
    let entry = hash_map.entry(key);
    match entry {
        Entry::Occupied(mut entry) => {
            let old_value = entry.insert(new_value);

            if old_value != new_value {
                Some(new_value)
            } else {
                None
            }
        },
        Entry::Vacant(entry) => {
            entry.insert(new_value);
            Some(new_value)
        }
    }
}

fn get_grid_map () -> (HashMap<u8, u32>, HashMap<u32, u8>) {
    let mut midi_to_id: HashMap<u8, u32> = HashMap::new();
    let mut id_to_midi: HashMap<u32, u8> = HashMap::new();

    for r in 0..8 {
        for c in 0..8 {
            let midi = ((8 - r) * 10 + c + 1) as u8;
            let id = (r * 8 + c) as u32;
            midi_to_id.insert(midi, id);
            id_to_midi.insert(id, midi);
        }
    } 

    (midi_to_id, id_to_midi)
}

fn get_transform (id: u32, sustained_values: &HashMap<u32, LoopTransform>, override_values: &HashMap<u32, LoopTransform>, selection: &HashSet<u32>, selection_override: &LoopTransform, loop_collection: &LoopCollection, override_collection: Option<&LoopCollection>, no_suppress: &HashSet<u32>) -> LoopTransform {
    let mut result = LoopTransform::None;

    let collection = if selection.contains(&id) && override_collection.is_some() {
        override_collection.unwrap()
    } else {
        loop_collection
    };

    if let Some(ref transform) = collection.transforms.get(&id) {
        result = transform.apply(&result);
    }

    if ((selection.len() == 0 && !no_suppress.contains(&id)) || selection.contains(&id)) && result.is_active() {
        result = selection_override.apply(&result);
    }

    let sustained_value = sustained_values.get(&id);

    // use the sustained value if override value is none
    // what a mess!
    if let Some(value) = override_values.get(&id) {
        result = if value == &LoopTransform::None {
            sustained_value.unwrap_or(&LoopTransform::None).apply(&result)
        } else {
            value.apply(&result)
        }
    } else if let Some(sustained_value) = sustained_value {
        result = sustained_value.apply(&result);
    }

    result
}

fn update_ids <'a> (a: &'a HashSet<u32>, b: &'a mut HashSet<u32>) -> (Vec<u32>, Vec<u32>) {
    let mut added = Vec::new();
    let mut removed = Vec::new();

    for id in a {
        if !b.contains(id) {
            added.push(*id)
        }
    }

    for id in b.iter() {
        if !a.contains(id) {
            removed.push(*id)
        }
    }

    for id in &added {
        b.insert(*id);
    }

    for id in &removed {
        b.remove(id);
    }

    (added, removed)
}

fn get_value (id: u32, position: MidiTime, align_offset: MidiTime, recorder: &LoopRecorder, transforms: &HashMap<u32, LoopTransform>, late_trigger: bool) -> Option<OutputValue> {
    match transforms.get(&id).unwrap_or(&LoopTransform::None) {
        &LoopTransform::Value(value) => Some(value),
        &LoopTransform::Range {pos: range_pos, length: range_length} => {
            let playback_offset = range_pos % range_length;
            let playback_pos = range_pos + ((position - playback_offset) % range_length);
            match recorder.get_event_at(id, playback_pos) {
                Some(event) if event.is_on() => {
                    match recorder.get_next_event_at(id, playback_pos) {
                        // don't force an output value if the next event is less than 1 beat away
                        Some(next_event) if (next_event.pos - playback_pos) < MidiTime::from_beats(1) => None,
                        _ => Some(event.value)
                    }
                },
                _ => Some(OutputValue::Off)
            }
        },
        &LoopTransform::Repeat { rate, offset, value } => {
            let pos = position - align_offset;
            if late_trigger {
                let mut current_on = pos.quantize(rate) + (offset % rate);
                if current_on > pos {
                    current_on = current_on - rate
                }
                if pos - current_on <= MidiTime::from_ticks(1) {
                    return Some(value);
                }
            }
            
            Some(OutputValue::Off)
        },
        _ => Some(OutputValue::Off)
    }
}

fn get_events (position: MidiTime, length: MidiTime, align_offset: MidiTime, recorder: &LoopRecorder, transforms: &HashMap<u32, LoopTransform>) -> Vec<LoopEvent> {
    let mut result = Vec::new();

    if length > MidiTime::zero() {        
        for (id, transform) in transforms {
            match transform {
                &LoopTransform::Range {pos: range_pos, length: range_length} => {
                    let playback_offset = range_pos % range_length;
                    let playback_pos = range_pos + ((position - playback_offset) % range_length);

                    if range_pos >= playback_pos && range_pos < (playback_pos + length) {
                        // insert start value
                        if let Some(value) = get_value(*id, range_pos, align_offset, recorder, transforms, false) {
                            LoopEvent {
                                id: *id, pos: position, value
                            }.insert_into(&mut result);
                        }
                    }

                    let offset = position - playback_pos;
                    if let Some(events) = recorder.get_range_for(*id, playback_pos, playback_pos + length) {
                        for event in events {
                            event.with_pos(event.pos + offset).insert_into(&mut result);
                        }
                    }
                },
                &LoopTransform::Repeat {rate: repeat_rate, offset: repeat_offset, value} => {
                    let next_on = next_repeat(position - align_offset, repeat_rate, repeat_offset) + align_offset;
                    let next_off = next_repeat(position - align_offset, repeat_rate, repeat_offset + repeat_rate.half()) + align_offset;
                    let to = position + length;


                    if next_on >= position && next_on < to {
                        LoopEvent {
                            value,
                            pos: next_on,
                            id: id.clone()
                        }.insert_into(&mut result);
                    }

                    if next_off >= position && next_off < to {
                        LoopEvent {
                            value: OutputValue::Off,
                            pos: next_off,
                            id: id.clone()
                        }.insert_into(&mut result);
                    }
                },
                _ => ()
            }
        }
    }

    result
}

#[derive(Clone, PartialEq, Eq)]
enum LaunchpadLight {
    Constant(Light),
    Pulsing(Light)
}

fn launchpad_text (text: &str) -> Vec<u8> {
    let prefix = [0xF0, 0x00, 0x20, 0x29, 0x02, 0x18, 0x14, 0x7C, 0x00];
    let suffix = [0xF7, 7];
    let mut result = Vec::new();
    result.extend_from_slice(&prefix);
    result.extend(String::from(text).into_bytes());
    result.extend_from_slice(&suffix);
    result
}

fn current_pos (last_pos: MidiTime, last_tick: SystemTime, tick_duration: Duration) -> MidiTime {
    let now = SystemTime::now();
    let since = now.duration_since(last_tick).unwrap();
    let offset_amount = (since.subsec_nanos() as f64) / (tick_duration.subsec_nanos() as f64);
    let ticks = offset_amount as i32;
    let fraction = ((offset_amount % 1.0) * 256.0) as u8;
    let offset_pos = MidiTime::new(ticks, fraction);
    last_pos + offset_pos
}

fn next_repeat (pos: MidiTime, rate: MidiTime, offset: MidiTime) -> MidiTime {
    let root = pos.quantize(rate) + (offset % rate);
    if root < pos {
        root + rate
    } else {
        root
    }
}

fn get_half_loop_length (time: MidiTime) -> MidiTime {
    let beats = time.as_float() / 24.0;
    let prev = prev_power_of_two((beats * 4.0) as u32) as f64 / 4.0;
    MidiTime::from_float(prev * 24.0)
}

fn get_double_loop_length (time: MidiTime) -> MidiTime {
    let beats = time.as_float() / 24.0;
    let next = next_power_of_two((beats * 4.0) as u32) as f64 / 4.0;
    MidiTime::from_float(next * 24.0)}

fn next_power_of_two(a: u32) -> u32 {
    let mut b = 1;
    while b <= a {
        b = b << 1;
    }
    return b;
}

fn prev_power_of_two(a: u32) -> u32 {
    let mut b = 1;
    while b < a {
        b = b << 1;
    }
    return b / 2;
}

fn get_repeat_for (chunk_id: usize, chunk_channels: &HashMap<usize, u32>, params: &Arc<Mutex<LoopGridParams>>) -> ChannelRepeat {
    let params = params.lock().unwrap();
    if let Some(channel) = chunk_channels.get(&chunk_id) {
        *params.channel_repeat.get(&channel).unwrap_or(&ChannelRepeat::None)
    } else {
        ChannelRepeat::None
    }
}

fn get_schedule_mode (id: u32, chunks: &Vec<Box<Triggerable>>, mapping: &HashMap<Coords, MidiMap>) -> ScheduleMode {
    if let Some(mapping) = mapping.get(&Coords::from(id)) {
        chunks.get(mapping.chunk_index).unwrap().schedule_mode()
    } else {
        ScheduleMode::MostRecent
    }
}

fn get_all_ids_in_this_chunk <'a> (id: u32, chunks: &Vec<Box<Triggerable>>, mapping: &HashMap<Coords, MidiMap>, chunk_trigger_ids: &'a Vec<Vec<u32>>) -> Vec<u32> {
    if let Some(mapping) = mapping.get(&Coords::from(id)) {
        chunk_trigger_ids.get(mapping.chunk_index).unwrap().clone()
    } else {
        Vec::new()
    }
}

fn pos_with_latency_compensation (tick_duration: Duration, pos: MidiTime, offset: Duration) -> MidiTime {
    pos - MidiTime::from_float(offset.subsec_nanos() as f64 / tick_duration.subsec_nanos() as f64)
}

fn commit_selection_override (selection_override_offset: &mut Option<isize>, loop_state: &mut LoopState, selection: &HashSet<u32>, last_changed_triggers: &mut HashMap<u32, MidiTime>, last_pos: &MidiTime) {
    // commit selection override offset
    if let Some(offset) = *selection_override_offset {
        if offset != 0 {
            let new_loop = if let Some(offset_loop) = loop_state.retrieve(offset) {
                let mut new_loop = loop_state.get().clone();
                for id in selection {
                    if let Some(transform) = offset_loop.transforms.get(id) {
                        new_loop.transforms.insert(*id, transform.clone());
                    } else {
                        new_loop.transforms.remove(id);
                    }
                    last_changed_triggers.insert(*id, *last_pos);
                }
                Some(new_loop)
            } else {
                None
            };
            
            if let Some(new_loop) = new_loop {
                loop_state.set(new_loop);
            }
        }

        *selection_override_offset = None;
    }
}

fn is_active (transform: &LoopTransform, id: &u32, loop_recorder: &LoopRecorder) -> bool {
    match transform {
        LoopTransform::Range {pos, length} => {
            let has_events = loop_recorder.has_events(*id, *pos, *pos + *length);
            let has_start_value = if let Some(event) = loop_recorder.get_event_at(*id, *pos) {
                event.is_on()
            } else { 
                true
            };

            has_events || has_start_value
        },
        _ => transform.is_active()
    }
}