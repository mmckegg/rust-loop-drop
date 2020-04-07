extern crate circular_queue;
extern crate midir;
use self::circular_queue::CircularQueue;
use std::time::{Instant, Duration};
use std::sync::mpsc;
use std::collections::HashSet;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::sync::{Arc, Mutex};
use std::cmp::Ordering;

use ::midi_connection;
use ::midi_time::MidiTime;
use ::scheduler;

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

enum TransformTarget {
    All,
    Main,
    Scale,
    Selected
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

enum LaunchpadEvent {
    LoopButton(bool),
    FlattenButton(bool),
    UndoButton(bool),
    RedoButton(bool),
    HoldButton(bool),
    SuppressButton(bool),
    ScaleButton(bool),
    SelectButton(bool),
    None,
    LengthButton {id: usize, pressed: bool},
    RateButton {id: usize, pressed: bool},
    BankButton {id: usize, pressed: bool},
    SampleButton {id: usize, pressed: bool},
    GridInput {id: u32, value: OutputValue, stamp: u64},
    GridPressure {id: u32, value: u8}
}

pub struct LoopGridLaunchpad {
    pub remote_tx: mpsc::Sender<LoopGridRemoteEvent>,
    remote_queue: mpsc::Receiver<LoopGridRemoteEvent>,
    
    _input: midi_connection::ThreadReference,
    params: Arc<Mutex<LoopGridParams>>,
    sample_channel: u8,
    sample_note_offset: u8,

    input_queue: mpsc::Receiver<LaunchpadEvent>,

    mapping: HashMap<Coords, MidiMap>,
    chunks: Vec<Box<Triggerable>>,
    chunk_colors: Vec<Light>,
    chunk_channels: HashMap<usize, u32>,
    chunk_trigger_ids: Vec<Vec<u32>>,
    sample_midi_port: midi_connection::SharedMidiOutputConnection,
    launchpad_output: midi_connection::SharedMidiOutputConnection,

    no_suppress: HashSet<u32>,
    trigger_latch_for: HashMap<usize, u32>,
    loop_length: MidiTime,

    repeating: bool,
    repeat_off_beat: bool,

    // selection
    selection_override: LoopTransform,
    selection: HashSet<u32>,
    suppressing: bool,
    holding: bool,
    holding_at: MidiTime,
    selecting: bool,
    selection_override_offset: Option<isize>,
    refresh_loop_length_in: Option<i32>,
    id_to_midi: HashMap<u32, u8>,

    loop_held: bool,
    loop_from: MidiTime,
    should_flatten: bool,

    selecting_scale: bool,
    selecting_scale_held: bool,
    last_selecting_scale: Instant,

    rate: MidiTime,
    recorder: LoopRecorder,

    last_pos: MidiTime,
    last_raw_pos: MidiTime,
    last_length: MidiTime,
    align_offset: MidiTime,

    current_bank: u8,

    sustained_values: HashMap<u32, LoopTransform>,
    override_values: HashMap<u32, LoopTransform>,
    input_values: HashMap<u32, OutputValue>,
    currently_held_inputs: Vec<u32>,
    currently_held_rates: Vec<usize>,
    last_changed_triggers: HashMap<u32, MidiTime>,

    frozen_loop: Option<LoopCollection>,

    // out state
    current_swing: f64,
    out_transforms: HashMap<u32, LoopTransform>,
    pending_repeat: HashMap<u32, LoopTransform>,
    out_values: HashMap<u32, OutputValue>,
    grid_out: HashMap<u32, LaunchpadLight>,
    select_out: Light,
    last_repeat_light_out: Light,
    last_triggered: HashMap<usize, CircularQueue<u32>>,

    // display state
    active: HashSet<u32>,
    recording: HashSet<u32>,
    clear_repeats: HashSet<u32>,

    last_beat_light: u8,
    last_repeat_light: u8,

    loop_state: LoopState
}

impl LoopGridLaunchpad {
    pub fn new(launchpad_port_name: &str, chunk_map: Vec<Box<ChunkMap>>, scale: Arc<Mutex<Scale>>, params: Arc<Mutex<LoopGridParams>>, sample_midi_port: midi_connection::SharedMidiOutputConnection, sample_channel: u8, sample_note_offset: u8) -> Self {
        let (midi_to_id, _id_to_midi) = get_grid_map();

        let (input_queue_tx, input_queue) = mpsc::channel();
        let (remote_tx, remote_queue) = mpsc::channel();

        let input = midi_connection::get_input(&launchpad_port_name, move |stamp, message| {
            if message[0] == 144 || message[0] == 128 {
                let grid_button = midi_to_id.get(&message[1]);
                if let Some(id) = grid_button {
                    let id = *id;
                    let value = if message[2] > 0 {
                        OutputValue::On(message[2])
                    } else {
                        OutputValue::Off
                    };

                    input_queue_tx.send(LaunchpadEvent::GridInput {stamp, id, value}).unwrap();
                } ;
            } else if message[0] == 160 { // poly aftertouch
                let grid_button = midi_to_id.get(&message[1]);
                if let Some(id) = grid_button {
                    input_queue_tx.send(LaunchpadEvent::GridPressure {id: *id, value: message[2]}).unwrap();
                }
            } else if message[0] == 176 {
                let pressed = message[2] > 0;

                if let Some(id) = LEFT_SIDE_BUTTONS.iter().position(|&x| x == message[1]) {
                    input_queue_tx.send(LaunchpadEvent::LengthButton {id, pressed}).unwrap();
                } else if let Some(id) = RIGHT_SIDE_BUTTONS.iter().position(|&x| x == message[1]) {
                    input_queue_tx.send(LaunchpadEvent::RateButton {id, pressed}).unwrap();
                } else if let Some(id) = TOP_BUTTONS.iter().position(|&x| x == message[1]) {
                    input_queue_tx.send(match id {
                        0 => LaunchpadEvent::LoopButton(pressed),
                        1 => LaunchpadEvent::FlattenButton(pressed),
                        2 => LaunchpadEvent::UndoButton(pressed),
                        3 => LaunchpadEvent::RedoButton(pressed),
                        4 => LaunchpadEvent::HoldButton(pressed),
                        5 => LaunchpadEvent::SuppressButton(pressed),
                        6 => LaunchpadEvent::ScaleButton(pressed),
                        7 => LaunchpadEvent::SelectButton(pressed),
                        _ => LaunchpadEvent::None
                    }).unwrap();
                } else if let Some(id) = BANK_BUTTONS.iter().position(|&x| x == message[1]) {
                    // use last 4 bottom buttons as bank switchers
                    input_queue_tx.send(LaunchpadEvent::BankButton {id, pressed}).unwrap();
                } else if let Some(id) = BOTTOM_BUTTONS.iter().position(|&x| x == message[1]) {
                    input_queue_tx.send(LaunchpadEvent::SampleButton {id, pressed}).unwrap();
                }
            }
        });

        let (_midi_to_id, id_to_midi) = get_grid_map();
        let mut loop_length = MidiTime::from_beats(8);
        let mut base_loop = LoopCollection::new(loop_length);

        let mut launchpad_output = midi_connection::get_shared_output(&launchpad_port_name);
        launchpad_output.on_connect(move |port| {
            // send sysex message to put launchpad into live mode
            port.send(&[0xF0, 0x00, 0x20, 0x29, 0x02, 0x10, 0x40, 0x2F, 0x6D, 0x3E, 0x0A, 0xF7]).unwrap();
        });

        let mut instance = LoopGridLaunchpad {
            _input: input,
            launchpad_output,
            loop_length,
            params,
            sample_channel,
            sample_note_offset,
            id_to_midi,

            // channels
            input_queue,
            remote_queue,
            remote_tx,
            
            mapping: HashMap::new(),
            chunks: Vec::new(),
            chunk_colors: Vec::new(),
            chunk_channels: HashMap::new(),
            chunk_trigger_ids: Vec::new(),
            sample_midi_port: sample_midi_port,

            no_suppress: HashSet::new(),
            trigger_latch_for: HashMap::new(),

            repeating: false,
            repeat_off_beat: false,

            // selection
            selection_override: LoopTransform::None,
            selection: HashSet::new(),
            suppressing: false,
            holding: false,
            holding_at: MidiTime::zero(),
            selecting: false,
            selection_override_offset: None,
            refresh_loop_length_in: None,

            loop_held: false,
            loop_from: MidiTime::from_ticks(0),
            should_flatten: false,

            selecting_scale: false,
            selecting_scale_held: false,
            last_selecting_scale: Instant::now(),

            rate: MidiTime::from_beats(2),
            recorder: LoopRecorder::new(),

            last_pos: MidiTime::from_ticks(0),
            last_raw_pos: MidiTime::from_ticks(0),
            last_length: MidiTime::from_ticks(0),
            align_offset: MidiTime::zero(),

            current_bank: 0,

            sustained_values: HashMap::new(),
            override_values: HashMap::new(),
            input_values: HashMap::new(),
            currently_held_inputs: Vec::new(),
            currently_held_rates: Vec::new(),
            last_changed_triggers: HashMap::new(),

            frozen_loop: None,

            // out state
            current_swing: 0.0,
            out_transforms: HashMap::new(),
            pending_repeat: HashMap::new(),
            out_values: HashMap::new(),
            grid_out: HashMap::new(),
            select_out: Light::Off,
            last_repeat_light_out: Light::Off,
            last_triggered: HashMap::new(),

            // display state
            active: HashSet::new(),
            recording: HashSet::new(),
            clear_repeats: HashSet::new(),

            last_beat_light: RIGHT_SIDE_BUTTONS[7],
            last_repeat_light: RIGHT_SIDE_BUTTONS[7],

            loop_state: LoopState::new(loop_length)
        };

        for item in chunk_map {
            let mut count = 0;
            let chunk_index = instance.chunks.len();
            let mut trigger_ids = Vec::new();
            for row in (item.coords.row)..(item.coords.row + item.shape.rows) {
                for col in (item.coords.col)..(item.coords.col + item.shape.cols) {
                    instance.mapping.insert(Coords::new(row, col), MidiMap {chunk_index, id: count});   
                    trigger_ids.push(Coords::id_from(row, col));                
                    count += 1;
                }
            }

            if item.chunk.latch_mode() == LatchMode::NoSuppress {
                for id in &trigger_ids {
                    instance.no_suppress.insert(*id);
                }
            }

            if let Some(active) = item.chunk.get_active() {
                for id in active {
                    if let Some(trigger_id) = trigger_ids.get(id as usize) {
                        if item.chunk.latch_mode() == LatchMode::LatchSingle {
                            instance.trigger_latch_for.insert(chunk_index, *trigger_id);
                        } else {
                            base_loop.transforms.insert(*trigger_id, LoopTransform::Value(OutputValue::On(100)));
                        }
                    }
                }
            }

            instance.chunk_trigger_ids.push(trigger_ids);
            instance.chunk_colors.push(Light::Value(item.color));

            if let Some(channel) = item.channel {
                instance.chunk_channels.insert(chunk_index, channel);
            }

            instance.chunks.push(item.chunk);
        }

        // create base level undo
        instance.loop_state.set(base_loop);

        instance.launchpad_output.send(&[176, TOP_BUTTONS[5], Light::RedLow.value()]).unwrap();
        instance.launchpad_output.send(&[176, TOP_BUTTONS[6], Light::BlueDark.value()]).unwrap();
        instance.refresh_loop_button();
        instance.refresh_undo_redo_lights();
        instance.refresh_selected_bank();

        for id in 0..128 {
            instance.refresh_grid_button(id);
        }

        instance
    }

    fn launchpad_input_event (&mut self, event: LaunchpadEvent) {
        match event {
            LaunchpadEvent::LoopButton(pressed) => {
                if self.selecting_scale && self.selecting {
                    if pressed {
                        self.tap_tempo();
                    }
                } else {
                    if pressed {
                        self.start_loop();
                    } else {
                        self.end_loop();
                    }
                }
            },
            LaunchpadEvent::FlattenButton(pressed) => {
                if pressed {
                    self.commit_selection_override();
                    if self.should_flatten {
                        self.flatten();
                    } else if self.selection.len() > 0 {
                        self.clear_loops(TransformTarget::Selected);
                    } else {
                        if self.selecting {
                            self.clear_loops(TransformTarget::All);
                            self.clear_automation()
                        } else {
                            if self.selecting_scale {
                                self.clear_loops(TransformTarget::Scale);
                            } else {
                                self.clear_loops(TransformTarget::Main);
                            }
                        }
                    }
                    self.clear_selection();
                }
            },
            LaunchpadEvent::UndoButton(pressed) => {
                if pressed {
                    if self.selecting && self.selecting_scale_held {
                        // nudge clock backwards (modify timing of existing loop)
                        // clock_sender.send(ToClock::Nudge(MidiTime::from_ticks(-1))).unwrap();
                    } else if self.selecting {
                        self.halve_loop_length();
                    } else if self.selection.len() > 0 {
                        self.undo_selection();
                    } else {
                        self.loop_state.undo();
                    }
                }
            },
            LaunchpadEvent::RedoButton(pressed) => {
                if pressed {
                    if self.selecting && self.selecting_scale_held {
                        // nudge clock forwards
                        // clock_sender.send(ToClock::Nudge(MidiTime::from_ticks(1))).unwrap();
                    } else if self.selecting { 
                        self.double_loop_length();
                    } else if self.selection.len() > 0 {
                        self.redo_selection();
                    } else {
                        self.loop_state.redo();
                    }
                }
            },
            LaunchpadEvent::HoldButton(pressed) => {
                self.holding = pressed;
                self.holding_at = self.last_pos;
                self.refresh_selection_override();
                self.refresh_should_flatten();
            },
            LaunchpadEvent::SuppressButton(pressed) => {
                self.suppressing = pressed;
                self.refresh_selection_override();
                self.refresh_should_flatten();
            },
            LaunchpadEvent::ScaleButton(pressed) => {
                if pressed {
                    self.last_selecting_scale = Instant::now();
                    self.selecting_scale = !self.selecting_scale;
                } else if self.last_selecting_scale.elapsed() > Duration::from_millis(300) {
                    self.selecting_scale = !self.selecting_scale;
                }

                self.selecting_scale_held = pressed;

                self.refresh_selecting_scale();
                self.refresh_undo_redo_lights();
            }
            LaunchpadEvent::SelectButton(pressed) => {
                self.selecting = pressed;
                if pressed {
                    self.clear_selection()
                }

                self.refresh_selecting_scale();
                self.refresh_select_state();
                self.refresh_undo_redo_lights();
            },
            LaunchpadEvent::LengthButton {id, pressed} => {
                if pressed {
                    if self.selecting {
                        // nudge align offset
                        let nudge_offset = ALIGN_OFFSET_NUDGES[id % ALIGN_OFFSET_NUDGES.len()];
                        self.nudge(nudge_offset);
                    } else {
                        self.set_loop_length(LOOP_LENGTHS[id % LOOP_LENGTHS.len()]);
                    }
                }
            },
            LaunchpadEvent::RateButton {id, pressed} => {
                let current_index = self.currently_held_rates.iter().position(|v| v == &id);

                if pressed && current_index == None {
                    self.currently_held_rates.push(id);
                } else if let Some(index) = current_index {
                    self.currently_held_rates.remove(index);
                }

                if self.currently_held_rates.len() > 0 {
                    let id = *self.currently_held_rates.iter().last().unwrap();
                    let rate = REPEAT_RATES[id as usize];
                    self.set_rate(rate);
                    self.repeat_off_beat = self.selecting;
                    self.repeating = id > 0 || self.repeat_off_beat;
                }
            },
            LaunchpadEvent::SampleButton {id, pressed} => {
                self.play_sample(id as u8, pressed);
            },
            LaunchpadEvent::BankButton {id, pressed} => {
                if pressed {
                    self.set_bank(id as u8)
                }
            },
            LaunchpadEvent::GridInput {id, value, stamp: _} => {
                self.grid_input(id, value);
            },
            LaunchpadEvent::GridPressure {id, value} => {
                // TODO: handle pressure
            },
            LaunchpadEvent::None => ()
        }
    }

    fn remote_event (&mut self, event: LoopGridRemoteEvent) {
        match event {
            LoopGridRemoteEvent::LoopButton(pressed) => {
                if pressed {
                    self.start_loop();
                } else {
                    self.end_loop();
                }
            },
            LoopGridRemoteEvent::DoubleButton(pressed) => {
                if pressed {
                    self.double_loop_length();
                }
            },
            LoopGridRemoteEvent::SustainButton(pressed) => {
                self.sustain_button(pressed);
            }
        }
    }

    fn drain_input_events (&mut self) {
        let loop_change_events: Vec<LoopStateChange> = self.loop_state.change_queue.try_iter().collect();
        for event in loop_change_events {
            self.loop_length = self.loop_state.get().length;
            self.initial_loop();
            self.refresh_active();
            self.refresh_loop_length();

            if event == LoopStateChange::Set {
                self.clear_recording();
            }
        }

        let launchpad_events: Vec<LaunchpadEvent> = self.input_queue.try_iter().collect();
        for event in launchpad_events {
            self.launchpad_input_event(event)
        }

        let remote_events: Vec<LoopGridRemoteEvent> = self.remote_queue.try_iter().collect();
        for event in remote_events {
            self.remote_event(event)
        }

        let bank = self.params.lock().unwrap().bank;
        if self.current_bank != bank {
            self.current_bank = bank;
            self.refresh_selected_bank();
        }
    }

    fn update_swing (&mut self) {
        let params = self.params.lock().unwrap();
        self.current_swing = params.swing;
    }

    pub fn schedule (&mut self, range: scheduler::ScheduleRange) {

        
        // only read swing on 8th notes to prevent back scheduling                          
        if range.ticked && range.from % MidiTime::from_ticks(12) == MidiTime::zero() {
            self.update_swing();
        }
        
        self.last_raw_pos = range.from;
        self.last_pos = (range.from - self.align_offset).swing(self.current_swing) + self.align_offset;
        self.last_length = (range.to - self.align_offset).swing(self.current_swing) + self.align_offset - self.last_pos;
        
        if range.jumped {
            self.initial_loop();
        }

        if range.ticked {
            
            // clear repeats from last cycle
            for id in &self.clear_repeats.clone() {
                self.pending_repeat.remove(id);
                if !self.currently_held_inputs.contains(&(id % 64)) {
                    self.refresh_override(*id);
                    self.refresh_grid_button(*id);
                }
            }
            self.clear_repeats.clear();
            
            // handle revert of loop length button
            if let Some(remain) = self.refresh_loop_length_in {
                if remain > 0 { 
                    self.refresh_loop_length_in = Some(remain - 1);
                } else {
                    self.refresh_loop_length_in = None;
                    self.refresh_loop_length();
                }
            }
            
            self.refresh_side_buttons();
            self.refresh_recording();
            self.chunk_tick();
            

        }

        // consume launchpad and other controllers
        self.drain_input_events();

        let mut events = self.get_events(self.last_pos, self.last_length);
        let mut ranked = HashMap::new();
        for (key, value) in &self.last_triggered {
            for id in value.iter() {
                *ranked.entry((key.clone(), id.clone())).or_insert(0) += 1;
            }
        }

        // sort events so that earlier defined chunks schedule first
        events.sort_by(|a, b| {
            let a_mapping = self.mapping.get(&Coords::from(a.id));
            let b_mapping = self.mapping.get(&Coords::from(b.id));
            if let Some(a_mapping) = a_mapping {
                if let Some(b_mapping) = b_mapping {
                    let chunk_cmp = a_mapping.chunk_index.cmp(&b_mapping.chunk_index);
                    let schedule_mode = self.chunks.get(a_mapping.chunk_index).unwrap().schedule_mode();
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
            if let Some(mapping) = self.mapping.get(&Coords::from(event.id)) {
                if event.value.is_on() {
                    self.last_triggered.entry(mapping.chunk_index).or_insert(CircularQueue::with_capacity(8)).push(event.id);
                }
                self.event(event);
            }
        }
    }

    fn play_sample (&mut self, id: u8, pressed: bool) {
        let velocity = if pressed {
            120
        } else {
            0
        };
        self.sample_midi_port.send(&[144 - 1 + self.sample_channel, self.sample_note_offset + id, velocity]).unwrap();
    }

    fn refresh_selected_bank (&mut self) {
        for (index, id) in BANK_BUTTONS.iter().enumerate() {
            if self.current_bank == index as u8 {
                self.launchpad_output.send(&[176, *id as u8, Light::White.value()]).unwrap();
            } else {
                self.launchpad_output.send(&[176, *id as u8, BANK_COLORS[index]]).unwrap();
            }
        }
    }

    fn refresh_side_buttons (&mut self) {
        let pos = self.last_pos - self.align_offset;

        let beat_display_multiplier = (24.0 * 8.0) / self.loop_length.ticks() as f64;
        let shifted_beat_position = (pos.ticks() as f64 * beat_display_multiplier / 24.0) as usize;

        let current_beat_light = RIGHT_SIDE_BUTTONS[shifted_beat_position % 8];
        let current_repeat_light = RIGHT_SIDE_BUTTONS[REPEAT_RATES.iter().position(|v| v == &self.rate).unwrap_or(0)];

        let rate_color = if self.repeat_off_beat { Light::RedMed } else { Light::YellowMed };

        if current_repeat_light != self.last_repeat_light || self.last_repeat_light_out != rate_color {
            self.launchpad_output.send(&[144, self.last_repeat_light, 0]).unwrap();
            self.launchpad_output.send(&[144, current_repeat_light, rate_color.value()]).unwrap();
        }

        let beat_start = pos.is_whole_beat();
        let base_last_beat_light = if current_repeat_light == self.last_beat_light {
            rate_color
        } else {
            Light::None
        };
        
        let base_beat_light = if current_repeat_light == current_beat_light {
            rate_color
        } else {
            Light::None
        };

        if current_beat_light != self.last_beat_light {
            self.launchpad_output.send(&[144, self.last_beat_light, base_last_beat_light.unwrap_or(Light::Off).value()]).unwrap();
            if !beat_start {
                self.launchpad_output.send(&[144, current_beat_light, base_beat_light.unwrap_or(Light::GreenLow).value()]).unwrap();
            }
        }

        if beat_start {
            self.launchpad_output.send(&[144, current_beat_light, Light::White.value()]).unwrap();
        } else if pos.beat_tick() == 3 {
            self.launchpad_output.send(&[144, current_beat_light, base_beat_light.unwrap_or(Light::GreenLow).value()]).unwrap();
        }

        self.last_beat_light = current_beat_light;
        self.last_repeat_light = current_repeat_light;
        self.last_repeat_light_out = rate_color;
    }

    fn refresh_undo_redo_lights (&mut self) {
        let color = if self.selecting_scale_held && self.selecting {
            // nudging
            Light::Orange
        } else if self.selecting {
            Light::GreenLow
        } else {
            Light::RedLow
        };

        self.launchpad_output.send(&[176, TOP_BUTTONS[2], color.value()]).unwrap();
        self.launchpad_output.send(&[176, TOP_BUTTONS[3], color.value()]).unwrap();
    }

    fn refresh_loop_button (&mut self) {
        self.launchpad_output.send(&[176, TOP_BUTTONS[0], Light::YellowMed.value()]).unwrap();
    }

    fn refresh_loop_length (&mut self) {
        for (index, id) in LEFT_SIDE_BUTTONS.iter().enumerate() {

            let prev_button_length = *LOOP_LENGTHS.get(index.wrapping_sub(1)).unwrap_or(&MidiTime::zero());
            let button_length = LOOP_LENGTHS[index];
            let next_button_length = *LOOP_LENGTHS.get(index + 1).unwrap_or(&(LOOP_LENGTHS[LOOP_LENGTHS.len() - 1] * 2));

            let result = if button_length == self.loop_length {
                Light::Yellow
            } else if self.loop_length < button_length && self.loop_length > prev_button_length {
                Light::Red
            } else if self.loop_length > button_length && self.loop_length < next_button_length {
                Light::Red
            } else {
                Light::Off
            };

            self.launchpad_output.send(&[176, *id, result.value()]).unwrap();
        }
    }

    fn set_bank (&mut self, id: u8) {
        let mut params = self.params.lock().unwrap();
        params.bank = id;
    }

    fn grid_input (&mut self, id: u32, value: OutputValue) {
        let current_index = self.currently_held_inputs.iter().position(|v| v == &id);
        let scale_id = id + 64;
        
        if value.is_on() && current_index == None {
            self.currently_held_inputs.push(id);
        } else if let Some(index) = current_index {
            self.currently_held_inputs.remove(index);
        }

        if self.selecting && value.is_on() {
            if self.selection.contains(&id) {
                self.selection.remove(&scale_id);
                self.selection.remove(&id);
            } else {
                if self.selecting_scale { // hack to avoid including drums/vox
                    self.selection.insert(scale_id);
                } else {
                    self.selection.insert(id);
                }
            }

            // range selection
            if self.currently_held_inputs.len() == 2 {
                let from = Coords::from(self.currently_held_inputs[0]);
                let to = Coords::from(self.currently_held_inputs[1]);

                let from_row = u32::min(from.row, to.row);
                let to_row = u32::max(from.row, to.row) + 1;
                let from_col = u32::min(from.col, to.col);
                let to_col = u32::max(from.col, to.col) + 1;

                for row in from_row..to_row {
                    for col in from_col..to_col {
                        let row_offset = if self.selecting_scale { 8 } else { 0 };
                        let id = Coords::id_from(row + row_offset, col);
                        self.selection.insert(id);
                        self.refresh_grid_button(id);
                    }
                }
            }

            self.refresh_grid_button(id);
        } else {
            let in_scale_view = (self.selecting_scale && (self.selection.len() == 0 || !self.selection.contains(&id))) || self.selection.contains(&scale_id);

            if in_scale_view  {
                self.input_values.insert(scale_id, value);
                self.input_values.remove(&id);
            } else {
                self.input_values.insert(id, value);
                self.input_values.remove(&(scale_id));
            }
            self.refresh_input(id);
            self.refresh_input(scale_id);
        }
        self.refresh_should_flatten();
    }

    fn refresh_input (&mut self, id: u32) {
        let mut value = self.input_values.get(&id).unwrap_or(&OutputValue::Off);
        let transform = match value {
            &OutputValue::On(velocity) => {
                if let Some(mapped) = self.mapping.get(&Coords::from(id)) {
                    match get_repeat_for(mapped.chunk_index, &self.chunk_channels, &self.params) {
                        ChannelRepeat::None => LoopTransform::Value(OutputValue::On(velocity)),
                        ChannelRepeat::Rate(rate) => LoopTransform::Repeat {rate, offset: MidiTime::zero(), value: OutputValue::On(velocity)},
                        ChannelRepeat::Global => {
                            if self.repeating {
                                let offset = if self.repeat_off_beat { self.rate / 2 } else { MidiTime::zero() };
                                LoopTransform::Repeat {rate: self.rate, offset, value: OutputValue::On(velocity)}
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

        let changed = match self.override_values.entry(id) {
            Occupied(mut entry) => {
                let different = entry.get() != &transform;
                entry.insert(transform.clone());
                different
            },
            Vacant(entry) => {
                let different = transform != LoopTransform::None;
                entry.insert(transform.clone());
                different
            }
        };

        if changed {

            if let LoopTransform::Repeat {..} = transform {
                self.pending_repeat.insert(id, transform.clone());
            }

            if get_schedule_mode(id, &self.chunks, &self.mapping) == ScheduleMode::Monophonic {
                // refresh all in this chunk if monophonic
                for id in get_all_ids_in_this_chunk(id, &self.chunks, &self.mapping, &self.chunk_trigger_ids) {
                    self.refresh_override(id);
                }
            } else if self.selection.contains(&id) {
                // refresh all in selection if part of selection
                for id in self.selection.clone() {
                    self.refresh_override(id);
                }
            } else {
                self.refresh_override(id);
            }
        }
    }
    
    fn refresh_override (&mut self, id: u32) {
        // use frozen loop if present
        let loop_collection = if let Some(frozen_loop) = &self.frozen_loop {
            frozen_loop
        } else {
            self.loop_state.get()
        };

        let selection_override_loop_collection = if self.frozen_loop.is_some() {
            None
        } else if let Some(offset) = self.selection_override_offset {
            self.loop_state.retrieve(offset)
        } else {
            None
        };

        let mut transform = get_transform(id, &self.sustained_values, &self.override_values, &self.selection, &self.selection_override, &loop_collection, selection_override_loop_collection, &self.pending_repeat, &self.no_suppress);

        // suppress if there are inputs held and monophonic scheduling
        if get_schedule_mode(id, &self.chunks, &self.mapping) == ScheduleMode::Monophonic && transform.is_active() {
            if !self.override_values.get(&id).unwrap_or(&LoopTransform::None).is_active() {
                // now check to see if any other triggers in the chunk have overrides
                let ids = get_all_ids_in_this_chunk(id, &self.chunks, &self.mapping, &self.chunk_trigger_ids);
                let chunk_has_override = ids.iter().any(|id| self.override_values.get(id).unwrap_or(&LoopTransform::None).is_active());
                if chunk_has_override {
                    // suppress this override
                    transform = LoopTransform::Value(OutputValue::Off);
                }
            }
        }

        // if this note is part of selection, and other notes in selection are being overridden, then suppress this trigger
        let selection_active = self.selection.iter().any(|x| self.override_values.get(x).unwrap_or(&LoopTransform::None).is_active());
        if transform.is_active() && !self.override_values.get(&id).unwrap_or(&LoopTransform::None).is_active() && self.selection.contains(&id) && selection_active {
            transform = LoopTransform::Value(OutputValue::Off);
        }

        if self.out_transforms.get(&id).unwrap_or(&LoopTransform::None).unwrap_or(&LoopTransform::Value(OutputValue::Off)) != transform.unwrap_or(&LoopTransform::Value(OutputValue::Off)) {
            self.out_transforms.insert(id, transform);

            self.last_changed_triggers.insert(id, self.last_pos);

            // send new value
            if let Some(value) = self.get_value(id, self.last_pos, false) {
                self.event(LoopEvent {
                    id, value, pos: self.last_pos
                });
            }
        }
    }

    fn refresh_grid_button (&mut self, id: u32) {
        let base_id = id % 64;

        let in_scale_view = (self.selecting_scale && (self.selection.len() == 0 || !self.selection.contains(&id))) || 
            (self.selecting && self.selecting_scale_held) || 
            self.selection.contains(&(base_id + 64));

        let (id, background_id) = if in_scale_view {
            (base_id + 64, base_id)
        } else {
            (base_id, base_id + 64)
        };

        let mapped = self.mapping.get(&Coords::from(id));
        let background_mapped = self.mapping.get(&Coords::from(background_id));

        let triggering = if self.out_values.get(&id).unwrap_or(&OutputValue::Off).is_on() {
            true
        } else if mapped.is_some() && self.trigger_latch_for.contains_key(&mapped.unwrap().chunk_index) {
            self.trigger_latch_for.get(&mapped.unwrap().chunk_index).unwrap() == &id
        } else {
            false
        };

        let background_triggering = if self.out_values.get(&background_id).unwrap_or(&OutputValue::Off).is_on() {
            true
        } else if background_mapped.is_some() && self.trigger_latch_for.contains_key(&background_mapped.unwrap().chunk_index) {
            self.trigger_latch_for.get(&background_mapped.unwrap().chunk_index).unwrap() == &background_id
        } else {
            false
        };

        let old_value = self.grid_out.remove(&base_id).unwrap_or(LaunchpadLight::Constant(Light::Off));

        let color = if let Some(mapped) = mapped {
            self.chunk_colors[mapped.chunk_index]
        } else {
            Light::Off
        };

        let selection_color = if in_scale_view {
            Light::Purple
        } else {
            Light::Green
        };

        let background_color = if let Some(background_mapped) = background_mapped {
            self.chunk_colors[background_mapped.chunk_index]
        } else {
            Light::Off
        };

        let new_value = if triggering && self.selection.contains(&id) {
            LaunchpadLight::Pulsing(Light::White)
        } else if triggering {
            LaunchpadLight::Constant(Light::White)
        } else if self.selection.contains(&id) {
            LaunchpadLight::Pulsing(selection_color)
        } else if self.recording.contains(&id) {
            LaunchpadLight::Pulsing(Light::RedLow)
        } else if background_triggering {
            LaunchpadLight::Constant(background_color)
        } else if self.active.contains(&id) {
            LaunchpadLight::Pulsing(color)
        } else {
            LaunchpadLight::Constant(color)
        };

        if new_value != old_value {
            let midi_id = self.id_to_midi.get(&base_id);
            let message = match new_value {
                LaunchpadLight::Constant(value) => [144, *midi_id.unwrap(), value.value()],
                LaunchpadLight::Pulsing(value) => [146, *midi_id.unwrap(), value.value()]
            };
            self.launchpad_output.send(&message).unwrap()
        }

        self.grid_out.insert(base_id, new_value);
    }

    fn refresh_selection_override (&mut self) {
        self.selection_override = if self.suppressing {
            LoopTransform::Value(OutputValue::Off)
        } else if self.holding {
            LoopTransform::Range {pos: self.holding_at, length: self.rate}
        } else {
            LoopTransform::None
        };

        for id in 0..128 {
            self.refresh_override(id);
        }
    }

    fn refresh_active (&mut self) {
        let current_loop = self.loop_state.get();
        let selection_override_loop_collection = if self.frozen_loop.is_some() {
            None
        } else if let Some(offset) = self.selection_override_offset {
            self.loop_state.retrieve(offset)
        } else {
            None
        };

        let mut ids = HashSet::new();
        for (id, transform) in &current_loop.transforms {
            if is_active(transform, id, &self.recorder) {
                ids.insert(*id);
            }
        }

        for id in &self.selection {
            if let Some(override_loop) = selection_override_loop_collection {
                if is_active(override_loop.transforms.get(id).unwrap_or(&LoopTransform::None), id, &self.recorder) {
                    ids.insert(*id);
                } else {
                    ids.remove(id);
                }
            }
        }

        let (added, removed) = update_ids(&ids, &mut self.active);

        for id in added {
            self.refresh_grid_button(id);
        }

        for id in removed {
            self.refresh_grid_button(id);
        }
    }

    fn refresh_recording (&mut self) {
        let mut ids = HashSet::new();

        let from = if self.loop_held {
            self.loop_from
        } else {
            self.last_pos - self.loop_length
        }; 

        for (id, last_changed) in &self.last_changed_triggers {
            if last_changed >= &from {
                ids.insert(*id);
            }
        }

        // for (id, value) in &self.override_values  {
        //     if value != &LoopTransform::None {
        //         ids.insert(*id);
        //     }
        // }

        let (added, removed) = update_ids(&ids, &mut self.recording);

        for id in added {
            self.refresh_grid_button(id);
        }

        for id in removed {
            self.refresh_grid_button(id);
        }
    }

    fn refresh_select_state (&mut self) {
        let new_state = if self.selecting {
            Light::Green
        } else if self.selection.len() > 0 {
            Light::GreenLow
        } else {
            Light::Off
        };

        if self.select_out != new_state {
            self.launchpad_output.send(&[178, TOP_BUTTONS[7], new_state.value()]).unwrap();
            self.select_out = new_state;
        }
    }

    fn event (&mut self, event: LoopEvent) {
        if let Some(mapped) = self.mapping.get(&Coords::from(event.id)).copied() {
            let new_value = event.value.clone();
            match maybe_update(&mut self.out_values, event.id, new_value) {
                Some(_) => {
                    if let Some(chunk) = self.chunks.get(mapped.chunk_index) {
                        if chunk.latch_mode() == LatchMode::LatchSingle && new_value.is_on() {
                            // track last triggered
                            if let Some(id) = self.trigger_latch_for.get(&mapped.chunk_index).copied() {
                                // queue refresh of previous trigger latch
                                self.refresh_grid_button(id);
                            }
                            self.trigger_latch_for.insert(mapped.chunk_index, event.id);
                        }
                    }
                    
                    self.refresh_grid_button(event.id);
                    self.trigger_chunk(mapped, new_value);
                },
                None => ()
            };
        
            self.recorder.add(event);

            // handle clearing of "early repeat" releasing
            if new_value.is_on() && self.pending_repeat.contains_key(&event.id) {
                self.clear_repeats.insert(event.id);
            }
        }
    }

    fn clear_recording (&mut self) {
        self.last_changed_triggers.clear();
    }

    fn tap_tempo (&mut self) {
        // TODO: make work
        // clock_sender.send(ToClock::TapTempo).unwrap();
    }

    fn start_loop (&mut self) {
        self.commit_selection_override();   
        self.loop_held = true;
        self.loop_from = self.last_pos.round();
        self.launchpad_output.send(&[176, TOP_BUTTONS[0], Light::Green.value()]).unwrap();
    }

    fn end_loop (&mut self) {
        self.loop_held = false;
        self.refresh_loop_button();
        let since_press = self.last_pos - self.loop_from;
        let threshold = MidiTime::from_ticks(20);
        let mut new_loop = self.loop_state.get().clone();

        if since_press > threshold {
            // loop range between loop button down and up
            let quantized_length = MidiTime::quantize_length(self.last_pos - self.loop_from);
            self.set_loop_length(quantized_length);
        } else {
            // loop range to loop button down using last loop_length
            self.loop_from = self.loop_from - self.loop_length
        }

        let mut recording_ids = HashSet::new();

        for (id, last_change) in &self.last_changed_triggers {
            if last_change > &self.loop_from {
                recording_ids.insert(*id);
            }
        }

        for (id, value) in &self.override_values {
            if value != &LoopTransform::None {
                recording_ids.insert(*id);
            }
        }

        for id in &self.selection {
            // include events in selection when looping 
            recording_ids.insert(*id);
        }

        for id in 0..128 {
            // include ids that are recording, or if self.selecting, all active IDs!
            let selected = self.selecting || self.selection.contains(&id);
            if recording_ids.contains(&id) || (selected && self.active.contains(&id)) {

                // only include in loop if there are items in the range
                let current_event = self.recorder.get_event_at(id, self.loop_from);
                let has_events = self.recorder.has_events(id, self.loop_from, self.loop_from + self.loop_length);
                if has_events || current_event.is_some() {
                    new_loop.transforms.insert(id, LoopTransform::Range {
                        pos: self.loop_from, 
                        length: self.loop_length
                    });
                } else {
                    new_loop.transforms.insert(id, LoopTransform::None);
                }
            }
        }

        if new_loop.transforms.len() > 0 {
            new_loop.length = self.loop_length;
            self.loop_state.set(new_loop);
            self.clear_recording();
        }

        self.clear_selection();
    }

    fn clear_selection (&mut self) {
        self.commit_selection_override();

        for id in self.selection.clone() {
            self.refresh_grid_button(id);
        }

        if !self.selecting_scale {
            self.selecting_scale = false;
            self.refresh_selecting_scale();
        }

        self.selection.clear();

        self.refresh_select_state();
        self.refresh_selection_override();
    }

    fn refresh_should_flatten (&mut self) {
        let new_value = &self.selection_override != &LoopTransform::None || self.override_values.values().any(|value| value != &LoopTransform::None) || self.sustained_values.len() > 0;
        if new_value != self.should_flatten {
            self.should_flatten = new_value;
            let color = if self.should_flatten {
                Light::GreenLow
            } else {
                Light::Off
            };
            self.launchpad_output.send(&[176, TOP_BUTTONS[1], color.value()]).unwrap();
        }
    }

    fn flatten (&mut self) {
        let mut new_loop = self.loop_state.get().clone();

        for id in 0..128 {
            let mut transform = self.out_transforms.get(&id).unwrap_or(&LoopTransform::None).clone();
            
            // check if there are actually events available for this range
            let is_empty = if let LoopTransform::Range {pos, length} = transform {
                !self.recorder.has_events(id, pos, pos + length)
            } else {
                false
            };

            if is_empty {
                new_loop.transforms.insert(id.clone(), LoopTransform::None);
            } else {
                new_loop.transforms.insert(id.clone(), transform);
            }
        }

        self.loop_state.set(new_loop);
    }

    fn clear_loops (&mut self, target: TransformTarget)  {
        let mut new_loop = self.loop_state.get().clone();

        let ids: Vec<u32> = match target {
            TransformTarget::All => (0..128).collect(),
            TransformTarget::Main => (0..128).collect(),
            TransformTarget::Scale => (0..128).collect(),
            TransformTarget::Selected => self.selection.iter().cloned().collect()
        };
        
        for id in ids {
            if !self.no_suppress.contains(&id) {
                new_loop.transforms.insert(id, LoopTransform::Value(OutputValue::Off));
            }
        }

        self.loop_state.set(new_loop);
    }

    fn clear_automation (&mut self) {
        let mut params = self.params.lock().unwrap();
        params.reset_automation = true;
    }

    fn double_loop_length (&mut self) {
        self.set_loop_length(get_double_loop_length(self.loop_length).min(MidiTime::from_beats(32)));
    }
    
    fn halve_loop_length (&mut self) {
        self.set_loop_length(get_half_loop_length(self.loop_length).max(MidiTime::from_measure(1, 4)));
    }

    fn undo_selection (&mut self) {
        if let Some(next_offset) = self.loop_state.previous_index_for(self.selection_override_offset.unwrap_or(0), &self.selection) {
            self.selection_override_offset = Some(next_offset);
            self.refresh_selection_override();
        }
    }

    fn redo_selection (&mut self) {
        if let Some(next_offset) = self.loop_state.next_index_for(self.selection_override_offset.unwrap_or(0), &self.selection) {
            self.selection_override_offset = Some(next_offset);
            self.refresh_selection_override();
        }
    }

    fn set_loop_length (&mut self, loop_length: MidiTime) {
        self.loop_length = loop_length;
        self.refresh_loop_length();
    }

    fn nudge (&mut self, nudge_offset: MidiTime) {
        self.align_offset = self.align_offset + nudge_offset;

        // flash offset amount
        if let Some(index) = ALIGN_OFFSET_NUDGES.iter().position(|x| x == &nudge_offset) {
            let iter = if index < 4 {
                index..4
            } else {
                4..(index + 1)
            };
    
            for index in iter {
                self.launchpad_output.send(&[176, LEFT_SIDE_BUTTONS[index], Light::Purple.value()]).unwrap();
            }

            self.refresh_loop_length_in = Some(nudge_offset.ticks().abs());
        }
    }

    fn sustain_button (&mut self, pressed: bool) {
        // send frozen to twister
        if pressed {
            let current_loop = self.loop_state.get();
            self.frozen_loop = Some(current_loop.clone());

            for (id, value) in &self.override_values {
                if value != &LoopTransform::None {
                    self.sustained_values.insert(*id, value.clone());
                }
            }
        } else {
            self.frozen_loop = None;
            self.sustained_values.clear();
        }

        for id in 0..128 {
            self.refresh_override(id);
        }

        self.refresh_should_flatten();

        let mut params = self.params.lock().unwrap();
        params.frozen = pressed;
    }

    fn refresh_selecting_scale (&mut self) {
        if self.selecting_scale {
            self.launchpad_output.send(&[178, TOP_BUTTONS[6], Light::Yellow.value()]).unwrap();    
        } else {
            self.launchpad_output.send(&[176, TOP_BUTTONS[6], Light::BlueDark.value()]).unwrap();
        };


        for id in 0..64 {
            self.refresh_grid_button(id);
        }
    }

    fn set_rate (&mut self, value: MidiTime) {
        self.rate = value;
        self.refresh_side_buttons();
        self.refresh_selection_override();

        let mut to_update = HashMap::new();
        for (id, value) in &self.override_values {
            if let Some(mapped) = self.mapping.get(&Coords::from(*id)) {
                if get_repeat_for(mapped.chunk_index, &self.chunk_channels, &self.params) == ChannelRepeat::Global {
                    if let &LoopTransform::Repeat {rate: _, offset, value} = value {
                        to_update.insert(*id, LoopTransform::Repeat {rate: self.rate, offset, value});
                    }
                }
            }
        }
        for (id, value) in to_update {
            self.override_values.insert(id, value);
            self.refresh_override(id);
        }
        
    }

    fn initial_loop (&mut self) {
        for id in 0..128 {
            let loop_collection = if let Some(frozen_loop) = &self.frozen_loop {
                frozen_loop
            } else {
                self.loop_state.get()
            };

            let selection_override_loop_collection = if let Some(offset) = self.selection_override_offset {
                self.loop_state.retrieve(offset)
            } else {
                None
            };
            let transform = get_transform(id, &self.sustained_values, &self.override_values, &self.selection, &self.selection_override, &loop_collection, selection_override_loop_collection, &self.pending_repeat, &self.no_suppress);
            
            if self.out_transforms.get(&id).unwrap_or(&LoopTransform::None) != &transform {
                self.out_transforms.insert(id, transform);
                self.last_changed_triggers.insert(id, self.last_pos);

                // send new value
                if let Some(value) = self.get_value(id, self.last_pos, false) {
                    self.event(LoopEvent {
                        id: id, value, pos: self.last_pos
                    });
                }
            }
        }
    }

    fn trigger_chunk (&mut self, map: MidiMap, value: OutputValue) {
        if let Some(chunk) = self.chunks.get_mut(map.chunk_index) {
            chunk.trigger(map.id, value);
        }
    }

    fn chunk_tick (&mut self) {
        for chunk in &mut self.chunks {
            chunk.on_tick(self.last_raw_pos);
        }
    }

    fn commit_selection_override (&mut self) {
        // commit selection override offset
        if let Some(offset) = self.selection_override_offset {
            if offset != 0 {
                let new_loop = if let Some(offset_loop) = self.loop_state.retrieve(offset) {
                    let mut new_loop = self.loop_state.get().clone();
                    for id in &self.selection {
                        if let Some(transform) = offset_loop.transforms.get(id) {
                            new_loop.transforms.insert(*id, transform.clone());
                        } else {
                            new_loop.transforms.remove(id);
                        }
                        self.last_changed_triggers.insert(*id, self.last_pos.clone());
                    }
                    Some(new_loop)
                } else {
                    None
                };
                
                if let Some(new_loop) = new_loop {
                    self.loop_state.set(new_loop);
                }
            }

            self.selection_override_offset = None;
        }
    }

    fn get_events (&self, position: MidiTime, length: MidiTime) -> Vec<LoopEvent> {
        let mut result = Vec::new();

        if length > MidiTime::zero() {        
            for (id, transform) in &self.out_transforms {
                match transform {
                    &LoopTransform::Range {pos: range_pos, length: range_length} => {
                        let playback_offset = range_pos % range_length;
                        let playback_pos = range_pos + ((position - playback_offset) % range_length);

                        if range_pos >= playback_pos && range_pos < (playback_pos + length) {
                            // insert start value
                            if let Some(value) = self.get_value(*id, range_pos, false) {
                                LoopEvent {
                                    id: *id, pos: position, value
                                }.insert_into(&mut result);
                            }
                        }

                        let offset = position - playback_pos;
                        if let Some(events) = self.recorder.get_range_for(*id, playback_pos, playback_pos + length) {
                            for event in events {
                                event.with_pos(event.pos + offset).insert_into(&mut result);
                            }
                        }
                    },
                    &LoopTransform::Repeat {rate: repeat_rate, offset: repeat_offset, value} => {
                        let next_on = next_repeat(position - self.align_offset, repeat_rate, repeat_offset) + self.align_offset;
                        let next_off = next_repeat(position - self.align_offset, repeat_rate, repeat_offset + repeat_rate.half()) + self.align_offset;
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

    fn get_value (&self, id: u32, position: MidiTime, late_trigger: bool) -> Option<OutputValue> {
        match self.out_transforms.get(&id).unwrap_or(&LoopTransform::None) {
            &LoopTransform::Value(value) => Some(value),
            &LoopTransform::Range {pos: range_pos, length: range_length} => {
                let playback_offset = range_pos % range_length;
                let playback_pos = range_pos + ((position - playback_offset) % range_length);
                match self.recorder.get_event_at(id, playback_pos) {
                    Some(event) if event.is_on() => {
                        match self.recorder.get_next_event_at(id, playback_pos) {
                            // don't force an output value if the next event is less than 1 beat away
                            Some(next_event) if (next_event.pos - playback_pos) < MidiTime::from_beats(1) => None,
                            _ => Some(event.value)
                        }
                    },
                    _ => Some(OutputValue::Off)
                }
            },
            &LoopTransform::Repeat { rate, offset, value } => {
                let pos = position - self.align_offset;
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

fn get_transform (id: u32, sustained_values: &HashMap<u32, LoopTransform>, override_values: &HashMap<u32, LoopTransform>, selection: &HashSet<u32>, selection_override: &LoopTransform, loop_collection: &LoopCollection, override_collection: Option<&LoopCollection>, pending_repeat: &HashMap<u32, LoopTransform>, no_suppress: &HashSet<u32>) -> LoopTransform {
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
    let pending_repeat_value = pending_repeat.get(&id);

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

    // handle triggering of "early repeat"
    if !result.is_active() && pending_repeat_value.is_some() {
        result = pending_repeat_value.unwrap().clone()
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