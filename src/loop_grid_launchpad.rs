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
use ::chunk::{Triggerable, MidiMap, ChunkMap, Coords, TriggerModeChange};
use ::scale::Scale;

const CHUNK_COLORS: [Light; 8] = [Light::Chunk1, Light::Chunk2, Light::Chunk3, Light::Chunk4, Light::Chunk5, Light::Chunk6, Light::Chunk7, Light::Chunk8];
const SIDE_BUTTONS: [u8; 8] = [89, 79, 69, 59, 49, 39, 29, 19];
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
}

pub struct LoopGridParams {
    pub swing: f64
}

#[derive(Debug, Copy, Clone)]
pub enum LoopGridMessage {
    Schedule(MidiTime, MidiTime),
    GridInput(u64, u32, OutputValue),
    LoopButton(bool),
    FlattenButton(bool),
    UndoButton(bool),
    RedoButton(bool),
    SuppressButton(bool),
    SelectButton(bool),
    HoldButton(bool),
    ScaleButton(bool),
    Event(LoopEvent),
    InitialLoop,
    ClearRecording,
    RefreshInput(u32),
    RefreshOverride(u32),
    RefreshGridButton(u32),
    RefreshSelectionOverride,
    RefreshSideButtons,
    RefreshShouldFlatten,
    RefreshActive,
    RefreshRecording,
    RefreshSelectState,
    ClearSelection,
    RefreshUndoRedoLights,
    SetRate(MidiTime),
    RateButton(usize, bool),
    TriggerChunk(MidiMap, OutputValue, SystemTime),
    ExternalInput(u32, OutputValue),
    TempoChanged(usize),
    RefreshSelectingScale,
    FlushChoke,
    None
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
enum Light {
    // http://launchpaddr.com/mk2palette/
    Yellow = 13,
    YellowMed = 97,
    Lime = 73,
    LimeLow = 63,
    Purple = 94,
    Green = 17,
    GreenMed = 76,
    GreenLow = 18,
    Orange = 96,
    OrangeMed = 126,
    OrangeLow = 105,
    Red = 72,
    RedMed = 120,
    RedLow = 6,
    White = 3,
    Off = 0,
    Chunk1 = 23, 
    Chunk2 = 39, 
    Chunk3 = 55, 
    Chunk4 = 35, 
    Chunk5 = 15, 
    Chunk6 = 43, 
    Chunk7 = 59,
    Chunk8 = 71, 
    None = -1
}

impl Light {
    pub fn unwrap_or (self, value: Light) -> Light {
        match self {
            Light::None => value,
            _ => self
        }
    }
}

pub struct LoopGridLaunchpad {
    _input: midi_connection::ThreadReference
}

impl LoopGridLaunchpad {
    pub fn new(launchpad_port_name: &str, chunk_map: Vec<Box<ChunkMap>>, scale: Arc<Mutex<Scale>>, params: Arc<Mutex<LoopGridParams>>, clock: RemoteClock) -> Self {
        let (tx, rx) = mpsc::channel();
        
        let tx_input =  mpsc::Sender::clone(&tx);
        let tx_clock =  mpsc::Sender::clone(&tx);
        let tx_feedback =  mpsc::Sender::clone(&tx);
        let tx_loop_state =  mpsc::Sender::clone(&tx);

        let (midi_to_id, _id_to_midi) = get_grid_map();

        let mut launchpad_output = midi_connection::get_shared_output(&launchpad_port_name);

        let input = midi_connection::get_input(&launchpad_port_name, move |stamp, message| {
            if message[0] == 144 || message[0] == 128 {
                let side_button = SIDE_BUTTONS.iter().position(|&x| x == message[1]);
                let grid_button = midi_to_id.get(&message[1]);
                if side_button.is_some() {
                    let rate_index = side_button.unwrap();
                    let active = message[2] > 0;
                    tx_input.send(LoopGridMessage::RateButton(rate_index, active)).unwrap();
                } else if grid_button.is_some() {
                    let value = if message[2] > 0 {
                        OutputValue::On(DEFAULT_VELOCITY)
                    } else {
                        OutputValue::Off
                    };

                    tx_input.send(LoopGridMessage::GridInput(stamp, *grid_button.unwrap(), value)).unwrap();
                } ;
            } else if message[0] == 176 {
                let active = message[2] > 0;
                let to_send = match message[1] {
                    104 => LoopGridMessage::LoopButton(active),
                    105 => LoopGridMessage::FlattenButton(active),
                    106 => LoopGridMessage::UndoButton(active),
                    107 => LoopGridMessage::RedoButton(active),
                    108 => LoopGridMessage::HoldButton(active),
                    109 => LoopGridMessage::SuppressButton(active),
                    110 => LoopGridMessage::ScaleButton(active),
                    111 => LoopGridMessage::SelectButton(active),
                    _ => LoopGridMessage::None
                };
                tx_input.send(to_send).unwrap();
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
                        tx_clock.send(LoopGridMessage::TempoChanged(value));
                    },
                    FromClock::Jump => {
                        tx_clock.send(LoopGridMessage::InitialLoop).unwrap();
                    }
                }
            }
        });

        thread::spawn(move || {
            let mut mapping: HashMap<Coords, MidiMap> = HashMap::new();
            let mut chunks: Vec<Box<Triggerable>> = Vec::new();
            let scale = scale;

            for mut item in chunk_map {
                let mut id = 0;
                let chunk_index = chunks.len();
                let mut trigger_ids = Vec::new();
                for row in (item.coords.row)..(item.coords.row + item.shape.rows) {
                    for col in (item.coords.col)..(item.coords.col + item.shape.cols) {
                        mapping.insert(Coords::new(row, col), MidiMap {chunk_index, id});   
                        trigger_ids.push(Coords::id_from(row, col));                
                        id += 1;
                    }
                }

                let tx_chunk_listener =  mpsc::Sender::clone(&tx_loop_state);

                item.chunk.listen(Box::new(move |index, value| {
                    if let Some(id) = trigger_ids.get(index as usize) {
                        tx_chunk_listener.send(LoopGridMessage::ExternalInput(*id, value)).unwrap();
                    }
                }));

                chunks.push(item.chunk);
            }


            let mut loop_length = MidiTime::from_beats(8);
            let mut loop_state = LoopState::new(loop_length, move |value, change| {
                loop_length = value.length;
                tx_loop_state.send(LoopGridMessage::InitialLoop).unwrap();
                tx_loop_state.send(LoopGridMessage::RefreshActive).unwrap();

                if change == LoopStateChange::Set {
                    tx_loop_state.send(LoopGridMessage::ClearRecording).unwrap();
                }
            });
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
            let mut loop_from = MidiTime::from_ticks(0);
            let mut should_flatten = false;

            let mut selecting_scale = false;

            let mut rate = MidiTime::from_beats(2);
            let mut recorder = LoopRecorder::new();
    
            let mut last_tick_at = SystemTime::now();
            let mut last_tick_durations: CircularQueue<Duration> = CircularQueue::with_capacity(12);
            let mut tick_duration = Duration::from_millis(60 / 120 / 24 * 1000);

            let mut last_pos = MidiTime::from_ticks(0);
            let mut override_values: HashMap<u32, LoopTransform> = HashMap::new();
            let mut input_values: HashMap<u32, OutputValue> = HashMap::new();
            let mut currently_held_inputs: Vec<u32> = Vec::new();
            let mut currently_held_rates: Vec<usize> = Vec::new();
            let mut last_changed_triggers: HashMap<u32, MidiTime> = HashMap::new();

            // out state
            let mut current_swing: f64 = 0.0;
            let mut out_transforms: HashMap<u32, LoopTransform> = HashMap::new();
            let mut out_values: HashMap<u32, OutputValue> = HashMap::new();
            let mut grid_out: HashMap<u32, LaunchpadLight> = HashMap::new();
            let mut select_out = Light::Off;
            let mut last_repeat_light_out = Light::Off;
            let mut last_scale_light_out = Light::Off;
            let mut last_triggered: HashMap<usize, CircularQueue<u32>> = HashMap::new();

            let mut last_choke_output = HashMap::new();
            let mut choke_queue = HashSet::new();

            // display state
            let mut active: HashSet<u32> = HashSet::new();
            let mut recording: HashSet<u32> = HashSet::new();

            let mut last_beat_light = SIDE_BUTTONS[7];
            let mut last_repeat_light = SIDE_BUTTONS[7];
            let mut last_scale_light = SIDE_BUTTONS[7];

            // default button lights
            launchpad_output.send(&[176, 104, Light::YellowMed as u8]).unwrap();
            launchpad_output.send(&[176, 109, Light::RedLow as u8]).unwrap();
            tx_feedback.send(LoopGridMessage::RefreshUndoRedoLights).unwrap();

            for id in 0..128 {
                tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
            }

            for received in rx {
                match received {
                    LoopGridMessage::Schedule(position, length) => {

                        // only read swing on 8th notes to prevent back scheduling
                        if position % MidiTime::from_ticks(12) == MidiTime::zero() {
                            let params = params.lock().unwrap();
                            current_swing = params.swing
                        }

                        let mut events = get_events_with_swing(position, length, &recorder, &out_transforms, current_swing);

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
                                    return if chunk_cmp == Ordering::Equal {
                                        ranked.get(&(b_mapping.chunk_index, b_mapping.id)).unwrap_or(&0).cmp(ranked.get(&(a_mapping.chunk_index, a_mapping.id)).unwrap_or(&0))
                                    } else {
                                        chunk_cmp
                                    }
                                }
                            }
                            a.id.cmp(&b.id)
                        });

                        for event in events {
                            if event.value.is_on() {
                                if let Some(mapping) = mapping.get(&Coords::from(event.id)) {
                                    last_triggered.entry(mapping.chunk_index).or_insert(CircularQueue::with_capacity(8)).push(event.id);
                                }
                            }
                            tx_feedback.send(LoopGridMessage::Event(event)).unwrap();
                        }

                        last_pos = position;
                        let current_time = SystemTime::now();
                        last_tick_durations.push(current_time.duration_since(last_tick_at).unwrap());
                        tick_duration = last_tick_durations.iter().sum::<Duration>() / (last_tick_durations.len() as u32);
                        last_tick_at = current_time;

                        tx_feedback.send(LoopGridMessage::RefreshSideButtons).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshRecording).unwrap();
                    },
                    LoopGridMessage::RefreshSideButtons => {
                        let current_scale = scale.lock().unwrap();
                        let beat_display_multiplier = (24.0 * 8.0) / loop_length.ticks() as f64;
                        let shifted_beat_position = (last_pos.ticks() as f64 * beat_display_multiplier / 24.0) as usize;

                        let current_beat_light = SIDE_BUTTONS[shifted_beat_position % 8];
                        let current_repeat_light = SIDE_BUTTONS[REPEAT_RATES.iter().position(|v| v == &rate).unwrap_or(0)];
                        let current_scale_light = *SIDE_BUTTONS.get(current_scale.scale as usize).unwrap_or(&SIDE_BUTTONS[0]);

                        let rate_color = if repeat_off_beat { Light::RedMed } else { Light::YellowMed };
                        let scale_color = Light::Chunk6;

                        if current_repeat_light != last_repeat_light || last_repeat_light_out != rate_color {
                            launchpad_output.send(&[144, last_repeat_light, 0]).unwrap();
                            launchpad_output.send(&[144, current_repeat_light, rate_color as u8]).unwrap();
                        }

                        if current_scale_light != last_scale_light || last_scale_light_out != scale_color {
                            launchpad_output.send(&[144, last_scale_light, 0]).unwrap();
                            launchpad_output.send(&[144, current_scale_light, scale_color as u8]).unwrap();
                        }

                        let beat_start = last_pos.is_whole_beat();

                        let base_last_beat_light = if current_repeat_light == last_beat_light {
                            rate_color
                        } else if current_scale_light == last_beat_light {
                            scale_color
                        } else {
                            Light::None
                        };
                        
                        let base_beat_light = if current_repeat_light == current_beat_light {
                            rate_color
                        } else if current_scale_light == current_beat_light {
                            scale_color
                        } else {
                            Light::None
                        };
 
                        if current_beat_light != last_beat_light {
                            launchpad_output.send(&[144, last_beat_light, base_last_beat_light.unwrap_or(Light::Off) as u8]).unwrap();
                            if !beat_start {
                                launchpad_output.send(&[144, current_beat_light, base_beat_light.unwrap_or(Light::GreenLow) as u8]).unwrap();
                            }
                        }

                        if beat_start {
                            launchpad_output.send(&[144, current_beat_light, Light::White as u8]).unwrap();
                        } else if last_pos.beat_tick() == 3 {
                            launchpad_output.send(&[144, current_beat_light, base_beat_light.unwrap_or(Light::GreenLow) as u8]).unwrap();
                        }

                        last_beat_light = current_beat_light;
                        last_repeat_light = current_repeat_light;
                        last_repeat_light_out = rate_color;
                        last_scale_light = current_scale_light;
                        last_scale_light_out = scale_color;
                    },
                    LoopGridMessage::RefreshUndoRedoLights => {
                        let color = if selecting_scale && selecting {
                            // nudging
                            Light::Orange
                        } else if selecting {
                            Light::GreenLow
                        } else if selecting_scale {
                            Light::YellowMed
                        } else {
                            Light::RedLow
                        };

                        launchpad_output.send(&[176, 106, color as u8]).unwrap();
                        launchpad_output.send(&[176, 107, color as u8]).unwrap();
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
                                if selecting_scale {
                                    selection.insert(scale_id);
                                } else {
                                    selection.insert(id);
                                }
                            }

                            // range selection
                            if currently_held_inputs.len() == 2 {

                                let row_offset = if selecting_scale { 8 } else { 0 };
                                let from = Coords::from(currently_held_inputs[0]);
                                let to = Coords::from(currently_held_inputs[1]);

                                let from_row = u32::min(from.row, to.row);
                                let to_row = u32::max(from.row, to.row) + 1;
                                let from_col = u32::min(from.col, to.col);
                                let to_col = u32::max(from.col, to.col) + 1;

                                for row in from_row..to_row {
                                    for col in from_col..to_col {
                                        let id = Coords::id_from(row + row_offset, col);
                                        selection.insert(id);
                                        tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
                                    }
                                }
                            }

                            tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
                        } else {
                            // switch to +64 if no selection or inside selection 
                            if (selecting_scale && (selection.len() == 0 || !selection.contains(&id))) || selection.contains(&scale_id) {
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
                                if repeating && (id < 64 || id >= 128) {
                                    let offset = if repeat_off_beat { rate / 2 } else { MidiTime::zero() };
                                    LoopTransform::Repeat {rate, offset, value: OutputValue::On(velocity)}
                                } else {
                                    LoopTransform::Value(OutputValue::On(velocity))
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
                            tx_feedback.send(LoopGridMessage::RefreshOverride(id)).unwrap();
                        }
                    },
                    LoopGridMessage::RefreshOverride(id) => {
                        let loop_collection = loop_state.get();
                        let transform = get_transform(id, &override_values, &selection, &selection_override, &loop_collection);
                        
                        if out_transforms.get(&id).unwrap_or(&LoopTransform::None).unwrap_or(&LoopTransform::Value(OutputValue::Off)) != transform.unwrap_or(&LoopTransform::Value(OutputValue::Off)) {
                            out_transforms.insert(id, transform);
                            last_changed_triggers.insert(id, last_pos);

                            let pos = current_pos(last_pos, last_tick_at, tick_duration);
 
                            // send new value
                            if let Some(value) = get_value(id, pos, &recorder, &out_transforms) {
                                tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                                    id, value, pos
                                })).unwrap();
                            }
                        }
                    },  
                    LoopGridMessage::TempoChanged(value) => {
                        launchpad_output.send(&launchpad_text(&value.to_string())).unwrap();
                    },
                    LoopGridMessage::RefreshGridButton(id) => {
                        if id < 128 {
                            
                            
                            let is_scale = id >= 64;
                            let id = id % 64;
                            let scale_id = id + 64;
                            
                            let out_value_scale = out_values.get(&scale_id).unwrap_or(&OutputValue::Off);
                            let out_value = out_values.get(&id).unwrap_or(&OutputValue::Off);
                            let old_value = grid_out.remove(&id).unwrap_or(LaunchpadLight::Constant(Light::Off));
                            
                            let in_scale_view = (selecting_scale && (selection.len() == 0 || !selection.contains(&id))) || selection.contains(&scale_id);

                            let base_color = if in_scale_view {
                                if let Some(mapped) = mapping.get(&Coords::from(scale_id)) {
                                    if active.contains(&id) {
                                        Light::Chunk8
                                    } else {
                                        CHUNK_COLORS[mapped.chunk_index]
                                    }
                                } else {
                                    Light::Off
                                }
                            } else {
                                if let Some(mapped) = mapping.get(&Coords::from(id)) {
                                    if active.contains(&scale_id) {
                                        Light::Chunk8
                                    } else {
                                        CHUNK_COLORS[mapped.chunk_index]
                                    }
                                } else {
                                    Light::Off
                                }
                            };

                            let triggering_scale_color = if let Some(mapped) = mapping.get(&Coords::from(scale_id)) {
                                if in_scale_view {
                                    Light::White
                                } else {
                                    CHUNK_COLORS[mapped.chunk_index]
                                }
                            } else {
                                Light::Off
                            };

                            let triggering_color = if let Some(mapped) = mapping.get(&Coords::from(id)) {
                                if in_scale_view {
                                    CHUNK_COLORS[mapped.chunk_index]
                                } else {
                                    Light::White
                                }
                            } else {
                                Light::Off
                            };

                            let new_value = if out_value != &OutputValue::Off {
                                LaunchpadLight::Constant(triggering_color)
                            } else if out_value_scale != &OutputValue::Off {
                                LaunchpadLight::Constant(triggering_scale_color)
                            } else if selection.contains(&id) {
                                LaunchpadLight::Constant(Light::Green)
                            } else if selection.contains(&scale_id) {
                            LaunchpadLight::Constant(Light::Purple)
                            } else if recording.contains(&id) || recording.contains(&scale_id) {
                                LaunchpadLight::Pulsing(Light::RedLow)
                            } else if active.contains(&id) {
                                LaunchpadLight::Pulsing(base_color)
                            } else {
                                LaunchpadLight::Constant(base_color)
                            };

                            if new_value != old_value {
                                let midi_id = id_to_midi.get(&id);
                                let message = match new_value {
                                    LaunchpadLight::Constant(value) => [144, *midi_id.unwrap(), value as u8],
                                    LaunchpadLight::Pulsing(value) => [146, *midi_id.unwrap(), value as u8]
                                };
                                launchpad_output.send(&message).unwrap()
                            }

                            grid_out.insert(id, new_value);
                        } else {
                            if let Some(mapped) = mapping.get(&Coords::from(id)) {
                                let value = active.contains(&id);
                                if let Some(chunk) = chunks.get(mapped.chunk_index) {
                                    chunk.onTriggerModeChanged(TriggerModeChange::Active(mapped.id, value));
                                }
                            }
                        }
                    },
                    LoopGridMessage::RefreshSelectionOverride => {
                        selection_override = if suppressing {
                            LoopTransform::Value(OutputValue::Off)
                        } else if holding {
                            LoopTransform::Range {pos: holding_at, length: rate}
                        } else {
                            LoopTransform::None
                        };

                        for id in 0..256 {
                            tx_feedback.send(LoopGridMessage::RefreshOverride(id)).unwrap();
                        }
                    },
                    LoopGridMessage::RefreshActive => {
                        let current_loop = loop_state.get();
                        let mut ids = HashSet::new();
                        for (id, transform) in &current_loop.transforms {
                            if transform.is_active() {
                                ids.insert(*id);
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

                        for (id, last_changed) in &last_changed_triggers {
                            if last_changed >= &(last_pos - loop_length) {
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
                            launchpad_output.send(&[178, 111, new_state as u8]).unwrap();
                            select_out = new_state;
                        }
                    },
                    LoopGridMessage::Event(event) => {
                        let new_value = event.value.clone();
                        let offset = event.pos - last_pos;
                        let fraction = (offset.frac() as f64) / 256.0;
                        let tick_nano = (tick_duration.subsec_nanos() as f64 * fraction) as u32;
                        let time = last_tick_at + Duration::new(0, tick_nano);
                        if let Some(mapped) = mapping.get(&Coords::from(event.id)) {
                            match maybe_update(&mut out_values, event.id, new_value) {
                                Some(_) => {
                                    tx_feedback.send(LoopGridMessage::RefreshGridButton(event.id)).unwrap();
                                    tx_feedback.send(LoopGridMessage::TriggerChunk(*mapped, new_value, time)).unwrap();
                                },
                                None => ()
                            };
                        }

                        let unswug_position = event.pos.swing(current_swing);
                        recorder.add(event.with_pos(unswug_position));
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
                                loop_from = last_pos;
                                launchpad_output.send(&[176, 104, Light::Green as u8]).unwrap();
                            } else {
                                launchpad_output.send(&[176, 104, Light::YellowMed as u8]).unwrap();
                                let since_press = last_pos - loop_from;
                                let threshold = MidiTime::from_ticks(20);
                                let mut new_loop = loop_state.get().clone();

                                if since_press > threshold {
                                    // loop range between loop button down and up
                                    let quantized_length = MidiTime::quantize_length(last_pos - loop_from);
                                    loop_length = quantized_length;
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

                                for id in 0..256 {
                                    // include ids that are recording, or if selecting, all active IDs!
                                    let selected = selecting || selection.contains(&id);
                                    if recording_ids.contains(&id) || (selected && active.contains(&id)) {
                                        new_loop.transforms.insert(id, LoopTransform::Range {
                                            pos: loop_from, 
                                            length: loop_length
                                        });
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
                        for id in &selection {
                            tx_feedback.send(LoopGridMessage::RefreshGridButton(*id)).unwrap();
                        }

                        if !selecting_scale {
                            selecting_scale = false;
                            tx_feedback.send(LoopGridMessage::RefreshSelectingScale).unwrap();
                        }

                        selection.clear();
                        tx_feedback.send(LoopGridMessage::RefreshSelectState).unwrap();
                    },
                    LoopGridMessage::RefreshShouldFlatten => {
                        let new_value = &selection_override != &LoopTransform::None || override_values.values().any(|value| value != &LoopTransform::None);
                        if new_value != should_flatten {
                            should_flatten = new_value;
                            let color = if should_flatten {
                                Light::GreenLow
                            } else {
                                Light::Off
                            };
                            launchpad_output.send(&[176, 105, color as u8]).unwrap();
                        }
                    },
                    LoopGridMessage::FlattenButton(pressed) => {
                        if pressed {
                            if should_flatten {
                                let mut new_loop = loop_state.get().clone();

                                for id in 0..256 {
                                    new_loop.transforms.insert(id.clone(), out_transforms.get(&id).unwrap_or(&LoopTransform::None).clone());
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

                                if selecting_scale {
                                    for id in 64..128 {
                                        // just erase scale events
                                        new_loop.transforms.insert(id, LoopTransform::Value(OutputValue::Off));
                                    }
                                } else {
                                    for id in 0..256 {
                                        new_loop.transforms.insert(id, LoopTransform::Value(OutputValue::Off));
                                    }
                                }
                                

                                loop_state.set(new_loop);
                            }
                            tx_feedback.send(LoopGridMessage::ClearSelection).unwrap();
                        }
                    },
                    LoopGridMessage::UndoButton(pressed) => {
                        if pressed {
                            if selecting && selecting_scale {
                                clock_sender.send(ToClock::Nudge(MidiTime::from_ticks(-1))).unwrap();
                            } else if selecting {
                                loop_length = (loop_length / 2).max(MidiTime::from_measure(1, 4));
                            } else if selecting_scale {
                                let mut current_scale = scale.lock().unwrap();
                                current_scale.root -= 1;
                            } else {
                                loop_state.undo();
                            }
                        }
                    },   
                    LoopGridMessage::RedoButton(pressed) => {
                        if pressed {
                            if selecting && selecting_scale {
                                clock_sender.send(ToClock::Nudge(MidiTime::from_ticks(1))).unwrap();
                            } else if selecting {
                                loop_length = (loop_length * 2).min(MidiTime::from_beats(32));
                            } else if selecting_scale {
                                let mut current_scale = scale.lock().unwrap();
                                current_scale.root += 1;
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
                        tx_feedback.send(LoopGridMessage::RefreshSelectState).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshUndoRedoLights).unwrap();
                    },
                    LoopGridMessage::ScaleButton(pressed) => {
                        selecting_scale = pressed;
                        tx_feedback.send(LoopGridMessage::RefreshSelectingScale).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshUndoRedoLights).unwrap();
                    },
                    LoopGridMessage::RefreshSelectingScale => {
                        let new_state = if selecting_scale {
                            Light::Yellow
                        } else {
                            Light::Off
                        };

                        launchpad_output.send(&[178, 110, new_state as u8]).unwrap();

                        for id in 0..64 {
                            tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
                        }

                        for chunk in &chunks {
                            chunk.onTriggerModeChanged(TriggerModeChange::SelectingScale(selecting_scale))
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
                            if selecting_scale {
                                let mut current_scale = scale.lock().unwrap();
                                current_scale.scale = id as i32;
                            } else {
                                let rate = REPEAT_RATES[id];
                                tx_feedback.send(LoopGridMessage::SetRate(rate)).unwrap();
                                repeat_off_beat = selecting;
                                repeating = id > 0 || repeat_off_beat;
                            }
                        }
                    },
                    LoopGridMessage::SetRate(value) => {
                        rate = value;
                        tx_feedback.send(LoopGridMessage::RefreshSideButtons).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshSelectionOverride).unwrap();

                        let mut to_update = HashMap::new();
                        for (id, value) in &override_values {
                            if let &LoopTransform::Repeat {rate: _, offset, value} = value {
                                to_update.insert(*id, LoopTransform::Repeat {rate, offset, value});
                            }
                        }
                        for (id, value) in to_update {
                            override_values.insert(id, value);
                            tx_feedback.send(LoopGridMessage::RefreshOverride(id)).unwrap();
                        }
                        
                    },                 
                    LoopGridMessage::InitialLoop => {
                        for id in 0..256 {
                            let loop_collection = loop_state.get();
                            let transform = get_transform(id, &override_values, &selection, &selection_override, &loop_collection);
                            
                            if out_transforms.get(&id).unwrap_or(&LoopTransform::None) != &transform {
                                out_transforms.insert(id, transform);
                                last_changed_triggers.insert(id, last_pos);

                                // send new value
                                if let Some(value) = get_value(id, last_pos, &recorder, &out_transforms) {
                                    tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                                        id: id, value, pos: last_pos
                                    })).unwrap();
                                }
                            }
                        }
                    },
                    LoopGridMessage::ExternalInput(id, value) => {
                        if selecting && value.is_on() && id >= 128 {
                            for i in 128..256 {
                                selection.insert(i);
                            }
                        } else {
                            input_values.insert(id, value);
                            tx_feedback.send(LoopGridMessage::RefreshInput(id)).unwrap();
                        }  
                    },
                    LoopGridMessage::TriggerChunk(map, value, time) => {
                        if let Some(chunk) = chunks.get_mut(map.chunk_index) {
                            if chunk.shouldChokeAll() {
                                match value {
                                    OutputValue::Off => {},
                                    OutputValue::On(_) => {
                                        // queue up choke for next cycle (fake event loop)
                                        if let Some(choke_id) = last_choke_output.get(&map.chunk_index) {
                                            choke_queue.insert((map.chunk_index, *choke_id));
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
            _input: input
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

fn get_transform (id: u32, override_values: &HashMap<u32, LoopTransform>, selection: &HashSet<u32>, selection_override: &LoopTransform, loop_collection: &LoopCollection) -> LoopTransform {
    let mut result = LoopTransform::None;

    if let Some(ref transform) = loop_collection.transforms.get(&id) {
        result = transform.apply(&result);
    }

    if (selection.len() == 0 || selection.contains(&id)) && result.is_active() {
        result = selection_override.apply(&result);
    }

    if let Some(value) = override_values.get(&id) {
        result = value.apply(&result);  
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

fn get_value (id: u32, position: MidiTime, recorder: &LoopRecorder, transforms: &HashMap<u32, LoopTransform>) -> Option<OutputValue> {
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
        _ => Some(OutputValue::Off)
    }
}

fn get_events_with_swing (position: MidiTime, length: MidiTime, recorder: &LoopRecorder, transforms: &HashMap<u32, LoopTransform>, swing: f64) -> Vec<LoopEvent> {
    if swing > 0.0 || swing < 0.0 {
        let swung_position = position.swing(swing);
        let swung_length = (position + length).swing(swing) - swung_position;

        get_events(swung_position, swung_length, recorder, transforms).iter().map(|event| {
            let offset = (event.pos - swung_position).as_float() / swung_length.as_float();
            let new_pos = MidiTime::from_float(offset * length.as_float()) + position;
            event.with_pos(new_pos)
        }).collect()
    } else {
        get_events(position, length, recorder, transforms)
    }
}

fn get_events (position: MidiTime, length: MidiTime, recorder: &LoopRecorder, transforms: &HashMap<u32, LoopTransform>) -> Vec<LoopEvent> {
    let mut result = Vec::new();

    if length > MidiTime::zero() {        
        for (id, transform) in transforms {
            match transform {
                &LoopTransform::Range {pos: range_pos, length: range_length} => {
                    let playback_offset = range_pos % range_length;
                    let playback_pos = range_pos + ((position - playback_offset) % range_length);

                    if range_pos >= playback_pos && range_pos < (playback_pos + length) {
                        // insert start value
                        if let Some(value) = get_value(*id, range_pos, recorder, transforms) {
                            LoopEvent {
                                id: *id, pos: position, value
                            }.insert_into(&mut result);
                        }
                    }

                    if let Some(events) = recorder.get_range_for(*id, playback_pos, playback_pos + length) {
                        for event in events {
                            event.with_pos(position).insert_into(&mut result);
                        }
                    }
                },
                &LoopTransform::Repeat {rate: repeat_rate, offset: repeat_offset, value} => {
                    let next_on = next_repeat(position, repeat_rate, repeat_offset);
                    let next_off = next_repeat(position, repeat_rate, repeat_offset + repeat_rate.half());
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
    MidiTime::new(ticks + last_pos.ticks(), fraction)
}

fn next_repeat (pos: MidiTime, rate: MidiTime, offset: MidiTime) -> MidiTime {
    let root = pos.quantize(rate) + (offset % rate);
    if root < pos {
        root + rate
    } else {
        root
    }
}