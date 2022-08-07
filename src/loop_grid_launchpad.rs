extern crate circular_queue;
extern crate midir;

use self::circular_queue::CircularQueue;
use std::cmp::Ordering;
use std::collections::hash_map::Entry;
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::atomic::{self, AtomicBool};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use midi_connection;
use midi_time::MidiTime;
use scheduler;

use chunk::{ChunkMap, Coords, LatchMode, MidiMap, RepeatMode, ScheduleMode, Triggerable};
use loop_recorder::{LoopEvent, LoopRecorder};
use loop_state::{LoopCollection, LoopState, LoopStateChange, LoopTransform};
use output_value::OutputValue;

const TOP_BUTTONS: [u8; 8] = [91, 92, 93, 94, 95, 96, 97, 98];
const RIGHT_SIDE_BUTTONS: [u8; 8] = [89, 79, 69, 59, 49, 39, 29, 19];
const LEFT_SIDE_BUTTONS: [u8; 8] = [80, 70, 60, 50, 40, 30, 20, 10];
const BOTTOM_BUTTONS: [u8; 8] = [101, 102, 103, 104, 105, 106, 107, 108];
const TRIGGER_MODE_BUTTONS: [u8; 4] = [1, 2, 3, 4];
const BANK_BUTTONS: [u8; 4] = [5, 6, 7, 8];
const BANK_COLORS: [u8; 4] = [17, 17, 17, 17];

const LOOP_BUTTON: u8 = TOP_BUTTONS[0];
const FLATTEN_BUTTON: u8 = TOP_BUTTONS[1];
const UNDO_BUTTON: u8 = TOP_BUTTONS[2];
const REDO_BUTTON: u8 = TOP_BUTTONS[3];
const HOLD_BUTTON: u8 = TOP_BUTTONS[4];
const SUPPRESS_BUTTON: u8 = TOP_BUTTONS[5];
const SESSION_BUTTON: u8 = TOP_BUTTONS[6];
const SHIFT_BUTTON: u8 = TOP_BUTTONS[7];

// THE LAUNCHPAD PRO MK3 IS JUST TOO DAMN SENSITIVE!
const VELOCITY_THRESHOLD: u8 = 20;

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
}

pub struct LoopGridParams {
    pub swing: f64,
    pub bank: u8,
    pub frozen: bool,
    pub cueing: bool,
    pub duck_triggered: bool,
    pub duck_tick_multiplier: f64,
    pub channel_triggered: HashSet<u32>,
    pub reset_automation: bool,
    pub reset_beat: u32,
    pub active_notes: HashSet<u8>,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum TriggerMode {
    Immediate = 0,
    Quantized = 1,
    Repeat = 2,
    Cycle = 3,
}

impl TriggerMode {
    fn from_id(id: usize) -> TriggerMode {
        match id {
            1 => TriggerMode::Quantized,
            2 => TriggerMode::Repeat,
            3 => TriggerMode::Cycle,
            _ => TriggerMode::Immediate,
        }
    }

    fn to_id(&self) -> usize {
        *self as usize
    }
}

#[derive(Debug, Copy, Clone)]
pub enum LoopGridRemoteEvent {
    DoubleButton(bool),
    LoopButton(bool),
    SustainButton(bool),
}

#[derive(Debug, Clone)]
struct RepeatState {
    phase: RepeatPhase,
    transform: LoopTransform,
    to: MidiTime,
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum RepeatPhase {
    None,
    Pending,
    QuantizePending,
    QuantizeCurrent,
    Current,
    Triggered,
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
    None,
}

enum TransformTarget {
    All,
    Main,
    Scale,
    Selected,
}

impl Light {
    pub fn unwrap_or(self, value: Light) -> Light {
        match self {
            Light::None => value,
            _ => self,
        }
    }

    pub fn value(&self) -> u8 {
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
            _ => 0,
        }
    }
}

enum LaunchpadEvent {
    Connected,

    LoopButton(bool),
    FlattenButton(bool),
    UndoButton(bool),
    RedoButton(bool),
    HoldButton(bool),
    SuppressButton(bool),
    ScaleButton(bool),
    ShiftButton(bool),
    SustainButton(bool),

    None,
    LengthButton { id: usize, pressed: bool },
    RateButton { id: usize, pressed: bool },
    TriggerModeButton { id: usize, pressed: bool },
    BankButton { id: usize, pressed: bool },
    GridInput { id: u32, value: u8, stamp: u64 },
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct CycleStep {
    id: u32,
    value: OutputValue,
    rate: MidiTime,
    offset: MidiTime,
}

pub struct LoopGridLaunchpad {
    pub remote_tx: mpsc::Sender<LoopGridRemoteEvent>,
    remote_queue: mpsc::Receiver<LoopGridRemoteEvent>,

    _input: midi_connection::ThreadReference,
    params: Arc<Mutex<LoopGridParams>>,
    use_internal_clock: Arc<AtomicBool>,

    input_queue: mpsc::Receiver<LaunchpadEvent>,

    mapping: HashMap<Coords, MidiMap>,
    chunks: Vec<Box<dyn Triggerable>>,
    chunk_colors: Vec<Light>,
    chunk_channels: HashMap<usize, u32>,
    chunk_trigger_ids: Vec<Vec<u32>>,
    launchpad_output: midi_connection::SharedMidiOutputConnection,

    no_suppress: HashSet<u32>,
    no_suppress_held: HashSet<u32>,
    chunk_repeat_mode: HashMap<usize, RepeatMode>,
    loop_length: MidiTime,

    repeat_off_beat: bool,

    // selection
    selection_override: LoopTransform,
    selection: HashSet<u32>,
    suppressing: bool,
    holding: bool,
    holding_at: MidiTime,
    shift_held: bool,
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

    current_bank: u8,

    sustained_values: HashMap<u32, LoopTransform>,
    override_values: HashMap<u32, LoopTransform>,
    input_values: HashMap<u32, OutputValue>,
    currently_held_inputs: Vec<u32>,
    currently_held_rates: Vec<usize>,
    last_changed_triggers: HashMap<u32, MidiTime>,

    // out state
    current_swing: f64,
    out_transforms: HashMap<u32, LoopTransform>,
    repeat_states: HashMap<u32, RepeatState>,

    out_values: HashMap<u32, OutputValue>,
    grid_out: HashMap<u32, LaunchpadLight>,
    bottom_button_out: HashMap<u32, LaunchpadLight>,
    select_out: Light,
    last_repeat_light_out: Light,
    last_triggered: HashMap<usize, CircularQueue<u32>>,

    trigger_mode: TriggerMode,
    chunk_cycle_step: HashMap<usize, CycleStep>,
    chunk_cycle_next_pos: HashMap<usize, MidiTime>,
    cycle_groups: HashMap<usize, Vec<CycleStep>>,

    // display state
    active: HashSet<u32>,
    recording: HashSet<u32>,

    last_beat_light: u8,
    last_repeat_light: u8,

    loop_state: LoopState,
}

impl LoopGridLaunchpad {
    pub fn new(
        launchpad_port_name: &str,
        chunk_map: Vec<Box<ChunkMap>>,
        params: Arc<Mutex<LoopGridParams>>,
        use_internal_clock: Arc<AtomicBool>,
    ) -> Self {
        let (midi_to_id, _id_to_midi) = get_grid_map();

        let (input_queue_tx, input_queue) = mpsc::channel();
        let (remote_tx, remote_queue) = mpsc::channel();

        let input_queue_connect_tx = input_queue_tx.clone();

        let input = midi_connection::get_input(&launchpad_port_name, move |stamp, message| {
            if message[0] == 144 || message[0] == 128 {
                let grid_button = midi_to_id.get(&message[1]);
                if let Some(id) = grid_button {
                    input_queue_tx
                        .send(LaunchpadEvent::GridInput {
                            stamp,
                            id: *id,
                            value: message[2],
                        })
                        .unwrap();
                };
            } else if message[0] == 160 {
                // poly aftertouch
                let grid_button = midi_to_id.get(&message[1]);
                if let Some(id) = grid_button {
                    input_queue_tx
                        .send(LaunchpadEvent::GridInput {
                            stamp,
                            id: *id,
                            value: message[2],
                        })
                        .unwrap();
                }
            } else if message[0] == 176 {
                let pressed = message[2] > 0;

                if let Some(id) = LEFT_SIDE_BUTTONS.iter().position(|&x| x == message[1]) {
                    input_queue_tx
                        .send(LaunchpadEvent::LengthButton { id, pressed })
                        .unwrap();
                } else if let Some(id) = RIGHT_SIDE_BUTTONS.iter().position(|&x| x == message[1]) {
                    input_queue_tx
                        .send(LaunchpadEvent::RateButton { id, pressed })
                        .unwrap();
                } else if let Some(id) = TOP_BUTTONS.iter().position(|&x| x == message[1]) {
                    input_queue_tx
                        .send(match id {
                            0 => LaunchpadEvent::LoopButton(pressed),
                            1 => LaunchpadEvent::FlattenButton(pressed),
                            2 => LaunchpadEvent::UndoButton(pressed),
                            3 => LaunchpadEvent::RedoButton(pressed),
                            4 => LaunchpadEvent::HoldButton(pressed),
                            5 => LaunchpadEvent::SuppressButton(pressed),
                            6 => LaunchpadEvent::ScaleButton(pressed),
                            7 => LaunchpadEvent::ShiftButton(pressed),
                            _ => LaunchpadEvent::None,
                        })
                        .unwrap();
                } else if let Some(id) = BANK_BUTTONS.iter().position(|&x| x == message[1]) {
                    // use last 4 bottom buttons as bank switchers
                    input_queue_tx
                        .send(LaunchpadEvent::BankButton { id, pressed })
                        .unwrap();
                } else if let Some(index) = BOTTOM_BUTTONS.iter().position(|&x| x == message[1]) {
                    let id = 128 + index as u32;
                    input_queue_tx
                        .send(LaunchpadEvent::GridInput {
                            stamp,
                            id,
                            value: message[2],
                        })
                        .unwrap();
                } else if let Some(id) = TRIGGER_MODE_BUTTONS.iter().position(|&x| x == message[1])
                {
                    input_queue_tx
                        .send(LaunchpadEvent::TriggerModeButton { id, pressed })
                        .unwrap();
                } else if message[1] == 90 {
                    // Shift Button
                    input_queue_tx
                        .send(LaunchpadEvent::SustainButton(pressed))
                        .unwrap();
                }
            }
        });

        let (_midi_to_id, id_to_midi) = get_grid_map();
        let loop_length = MidiTime::from_beats(8);
        let mut base_loop = LoopCollection::new(loop_length);

        let mut launchpad_output = midi_connection::get_shared_output(&launchpad_port_name);
        launchpad_output.on_connect(move |port| {
            port.send(&[0xF0, 0x00, 0x20, 0x29, 0x02, 0x0E, 0x0E, 0x01, 0xF7])
                .unwrap();

            input_queue_connect_tx
                .send(LaunchpadEvent::Connected)
                .unwrap();
        });

        let mut instance = LoopGridLaunchpad {
            _input: input,
            launchpad_output,
            loop_length,
            params,
            use_internal_clock,
            id_to_midi,

            trigger_mode: TriggerMode::Immediate,
            chunk_cycle_step: HashMap::new(),
            chunk_cycle_next_pos: HashMap::new(),
            cycle_groups: HashMap::new(),

            // channels
            input_queue,
            remote_queue,
            remote_tx,

            chunk_repeat_mode: HashMap::new(),
            mapping: HashMap::new(),
            chunks: Vec::new(),
            chunk_colors: Vec::new(),
            chunk_channels: HashMap::new(),
            chunk_trigger_ids: Vec::new(),

            no_suppress: HashSet::new(),
            no_suppress_held: HashSet::new(),

            repeat_off_beat: false,

            // selection
            selection_override: LoopTransform::None,
            selection: HashSet::new(),
            suppressing: false,
            holding: false,
            holding_at: MidiTime::zero(),
            shift_held: false,
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

            current_bank: 0,

            sustained_values: HashMap::new(),
            override_values: HashMap::new(),
            input_values: HashMap::new(),
            currently_held_inputs: Vec::new(),
            currently_held_rates: Vec::new(),
            last_changed_triggers: HashMap::new(),

            // out state
            current_swing: 0.0,
            out_transforms: HashMap::new(),
            repeat_states: HashMap::new(),

            out_values: HashMap::new(),
            grid_out: HashMap::new(),
            bottom_button_out: HashMap::new(),
            select_out: Light::Off,
            last_repeat_light_out: Light::Off,
            last_triggered: HashMap::new(),

            // display state
            active: HashSet::new(),
            recording: HashSet::new(),

            last_beat_light: RIGHT_SIDE_BUTTONS[7],
            last_repeat_light: RIGHT_SIDE_BUTTONS[7],

            loop_state: LoopState::new(loop_length),
        };

        for item in chunk_map {
            let mut count = 0;
            let chunk_index = instance.chunks.len();
            let mut trigger_ids = Vec::new();
            for row in (item.coords.row)..(item.coords.row + item.shape.rows) {
                for col in (item.coords.col)..(item.coords.col + item.shape.cols) {
                    instance.mapping.insert(
                        Coords::new(row, col),
                        MidiMap {
                            chunk_index,
                            id: count,
                        },
                    );
                    trigger_ids.push(Coords::id_from(row, col));
                    // preallocate memory for 50,000 recorded events per channel
                    instance.recorder.allocate(count, 50000);
                    count += 1;
                }
            }

            if item.chunk.latch_mode() == LatchMode::NoSuppress {
                for id in &trigger_ids {
                    instance.no_suppress.insert(*id);
                }
            } else if item.chunk.latch_mode() == LatchMode::LatchSuppress {
                for id in &trigger_ids {
                    instance.no_suppress_held.insert(*id);
                }
            }

            if item.repeat_mode == RepeatMode::OnlyQuant {
                for id in &trigger_ids {
                    instance.no_suppress_held.insert(*id);
                }
            }

            instance.chunk_trigger_ids.push(trigger_ids);
            instance.chunk_colors.push(Light::Value(item.color));
            instance
                .chunk_repeat_mode
                .insert(chunk_index, item.repeat_mode);

            if let Some(channel) = item.channel {
                instance.chunk_channels.insert(chunk_index, channel);
            }

            instance.chunks.push(item.chunk);
        }

        // create base level undo
        instance.loop_state.set(base_loop);

        instance
    }

    fn refresh_grid_buttons(&mut self) {
        for id in 0..136 {
            self.refresh_grid_button(id);
        }
    }

    fn launchpad_input_event(&mut self, event: LaunchpadEvent) {
        match event {
            LaunchpadEvent::Connected => {
                println!("Launchpad Connected");
                self.grid_out.clear();
                self.bottom_button_out.clear();
                self.refresh_grid_buttons();
                self.launchpad_output.send(&[176, HOLD_BUTTON, 32]).unwrap();
                self.launchpad_output
                    .send(&[176, SUPPRESS_BUTTON, 57])
                    .unwrap();
                self.launchpad_output
                    .send(&[176, SESSION_BUTTON, 45])
                    .unwrap();
                self.refresh_loop_button();
                self.refresh_undo_redo_lights();
                self.refresh_selected_bank();
                self.refresh_selected_trigger_mode();
            }
            LaunchpadEvent::LoopButton(pressed) => {
                if self.selecting_scale && self.shift_held {
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
            }
            LaunchpadEvent::FlattenButton(pressed) => {
                if pressed {
                    self.commit_selection_override();
                    if self.should_flatten {
                        self.flatten();
                    } else if self.selection.len() > 0 {
                        self.clear_loops(TransformTarget::Selected, true);
                    } else {
                        if self.shift_held {
                            self.clear_loops(TransformTarget::All, false);
                            self.clear_automation();
                        } else {
                            if self.selecting_scale {
                                self.clear_loops(TransformTarget::Scale, false);
                            } else {
                                self.clear_loops(TransformTarget::Main, false);
                            }
                        }
                    }
                    self.clear_selection();
                }
            }
            LaunchpadEvent::UndoButton(pressed) => {
                if pressed {
                    if self.shift_held {
                        self.halve_loop_length();
                    } else if self.selection.len() > 0 {
                        self.undo_selection();
                    } else {
                        self.loop_state.undo();
                    }
                }
            }
            LaunchpadEvent::RedoButton(pressed) => {
                if pressed {
                    if self.shift_held {
                        self.double_loop_length();
                    } else if self.selection.len() > 0 {
                        self.redo_selection();
                    } else {
                        self.loop_state.redo();
                    }
                }
            }
            LaunchpadEvent::HoldButton(pressed) => {
                self.holding = pressed;
                self.holding_at = self.last_pos;
                self.refresh_selection_override();
                self.refresh_should_flatten();
            }
            LaunchpadEvent::SuppressButton(pressed) => {
                self.suppressing = pressed;
                self.refresh_selection_override();
                self.refresh_should_flatten();
            }
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
            LaunchpadEvent::ShiftButton(pressed) => {
                self.shift_held = pressed;
                if pressed {
                    self.clear_selection()
                }

                self.refresh_selecting_scale();
                self.refresh_select_state();
                self.refresh_undo_redo_lights();
            }
            LaunchpadEvent::LengthButton { id, pressed } => {
                if pressed {
                    self.set_loop_length(LOOP_LENGTHS[id % LOOP_LENGTHS.len()]);
                }
            }
            LaunchpadEvent::RateButton { id, pressed } => {
                let current_index = self.currently_held_rates.iter().position(|v| v == &id);

                if pressed && current_index == None {
                    self.currently_held_rates.push(id);
                } else if let Some(index) = current_index {
                    self.currently_held_rates.remove(index);
                }

                if self.currently_held_rates.len() > 0 {
                    let id = *self.currently_held_rates.iter().last().unwrap();
                    let rate = REPEAT_RATES[id as usize];
                    self.repeat_off_beat = self.shift_held;
                    self.set_rate(rate);
                }
            }
            LaunchpadEvent::TriggerModeButton { id, pressed } => {
                if pressed {
                    self.set_trigger_mode(TriggerMode::from_id(id));
                }
            }
            LaunchpadEvent::BankButton { id, pressed } => {
                if pressed {
                    if id == 3 && self.shift_held {
                        self.toggle_internal_clock();
                    } else {
                        self.set_bank(id as u8);
                    }
                }
            }
            LaunchpadEvent::GridInput {
                id,
                value,
                stamp: _,
            } => {
                let value = adjust_velocity(value);
                if value > 0 {
                    self.grid_input(id, OutputValue::On(value));
                } else {
                    self.grid_input(id, OutputValue::Off);
                }
            }
            LaunchpadEvent::SustainButton(pressed) => {
                self.freeze_button(pressed);
            }
            LaunchpadEvent::None => (),
        }
    }

    fn remote_event(&mut self, event: LoopGridRemoteEvent) {
        match event {
            LoopGridRemoteEvent::LoopButton(pressed) => {
                if pressed {
                    self.start_loop();
                } else {
                    self.end_loop();
                }
            }
            LoopGridRemoteEvent::DoubleButton(pressed) => {
                if pressed {
                    self.double_loop_length();
                }
            }
            LoopGridRemoteEvent::SustainButton(pressed) => {
                self.freeze_button(pressed);
            }
        }
    }

    fn drain_input_events(&mut self) {
        let loop_change_events: Vec<LoopStateChange> =
            self.loop_state.change_queue.try_iter().collect();
        for event in loop_change_events {
            self.initial_loop();
            self.refresh_active();
            self.refresh_loop_length();
            self.refresh_cycle_groups();

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

    fn update_swing(&mut self) {
        let params = self.params.lock().unwrap();
        self.current_swing = params.swing;
    }

    pub fn schedule(&mut self, range: scheduler::ScheduleRange) {
        // only read swing on 8th notes to prevent back scheduling
        if range.ticked && range.to.floor() % MidiTime::from_ticks(12) == MidiTime::zero() {
            self.update_swing();
        }

        self.last_raw_pos = range.from;
        self.last_pos = range.from.swing(self.current_swing);
        self.last_length = range.to.swing(self.current_swing) - self.last_pos;

        if range.jumped {
            self.initial_loop();
        }

        if range.ticked {
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
        }

        // consume launchpad and other controllers
        self.drain_input_events();

        // clear repeats from last cycle
        let mut to_refresh = Vec::new();

        for (id, repeat_state) in &mut self.repeat_states {
            if repeat_state.phase != RepeatPhase::None && self.last_pos >= repeat_state.to {
                if let Some(LoopTransform::Repeat { rate, offset, .. }) =
                    self.override_values.get(&id)
                {
                    repeat_state.to =
                        next_repeat(self.last_pos + MidiTime::from_sub_ticks(1), *rate, *offset);
                    repeat_state.phase = RepeatPhase::Current;
                } else if let Some(LoopTransform::Cycle { rate, offset, .. }) =
                    self.override_values.get(&id)
                {
                    repeat_state.to =
                        next_repeat(self.last_pos + MidiTime::from_sub_ticks(1), *rate, *offset);
                    repeat_state.phase = RepeatPhase::Current;
                } else if let Some(LoopTransform::Value { .. }) = self.override_values.get(&id) {
                    // extend quantize
                    repeat_state.to = repeat_state.to + self.rate - MidiTime::from_sub_ticks(1);
                    repeat_state.phase = RepeatPhase::QuantizeCurrent;
                    to_refresh.push(id.clone());
                } else if repeat_state.phase == RepeatPhase::QuantizeCurrent {
                    // handle quantize end
                    repeat_state.phase = RepeatPhase::None;
                    to_refresh.push(id.clone());
                } else if let LoopTransform::Value { .. } = repeat_state.transform {
                    // detect quantize start
                    repeat_state.to = repeat_state.to + self.rate - MidiTime::from_sub_ticks(1);
                    repeat_state.phase = RepeatPhase::QuantizeCurrent;
                    to_refresh.push(id.clone());
                } else {
                    repeat_state.phase = RepeatPhase::None;
                    if !self.currently_held_inputs.contains(&(id % 64)) {
                        to_refresh.push(id.clone())
                    }
                }
            }
        }

        for id in to_refresh {
            self.refresh_override(id);
        }

        let mut events = self.get_events();
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
                    let schedule_mode = self
                        .chunks
                        .get(a_mapping.chunk_index)
                        .unwrap()
                        .schedule_mode();
                    return if chunk_cmp == Ordering::Equal
                        && schedule_mode == ScheduleMode::Percussion
                    {
                        ranked
                            .get(&(b_mapping.chunk_index, b_mapping.id))
                            .unwrap_or(&0)
                            .cmp(
                                ranked
                                    .get(&(a_mapping.chunk_index, a_mapping.id))
                                    .unwrap_or(&0),
                            )
                    } else {
                        chunk_cmp
                    };
                }
            }
            a.id.cmp(&b.id)
        });

        let mut chunks_needing_tick: HashSet<usize> = if range.ticked {
            let mut ids = HashSet::new();
            for id in 0..self.chunks.len() {
                ids.insert(id);
            }
            ids
        } else {
            HashSet::new()
        };

        for event in events {
            if let Some(mapping) = self.mapping.get(&Coords::from(event.id)).cloned() {
                // we run the chunk tick just before scheduling begins for that chunk
                // since chunks are scheduled in the order that they are added via config, this means that modulator
                // chunks can be scheduled before the triggers and the modulations for the same tick will be sent immediately
                if chunks_needing_tick.contains(&mapping.chunk_index) {
                    self.chunk_tick(mapping.chunk_index);
                    chunks_needing_tick.remove(&mapping.chunk_index);
                }
                if event.value.is_on() {
                    self.last_triggered
                        .entry(mapping.chunk_index)
                        .or_insert(CircularQueue::with_capacity(8))
                        .push(event.id);
                }
                self.event(event);
            }
        }

        self.update_cycle_steps();

        // schedule any remaining chunk ticks
        for index in chunks_needing_tick {
            self.chunk_tick(index);
        }

        self.refresh_active_notes();

        // refresh grid lights
        for id in 0..136 {
            self.refresh_grid_button(id);
        }
    }

    fn refresh_active_notes(&mut self) {
        let mut params = self.params.lock().unwrap();
        params.active_notes.clear();

        for chunk in &self.chunks {
            let notes = chunk.get_notes();
            if let Some(notes) = notes {
                for note in notes {
                    params.active_notes.insert(note);
                }
            }
        }
    }

    fn refresh_selected_bank(&mut self) {
        let bank_color = if self.use_internal_clock.load(atomic::Ordering::Relaxed) {
            95
        } else {
            17
        };
        for (index, id) in BANK_BUTTONS.iter().enumerate() {
            if self.current_bank == index as u8 {
                self.launchpad_output
                    .send(&[178, *id as u8, Light::White.value()])
                    .unwrap();
            } else {
                self.launchpad_output
                    .send(&[176, *id as u8, bank_color])
                    .unwrap();
            }
        }
    }

    fn refresh_selected_trigger_mode(&mut self) {
        for (index, id) in TRIGGER_MODE_BUTTONS.iter().enumerate() {
            if self.trigger_mode.to_id() == index {
                self.launchpad_output
                    .send(&[178, *id as u8, Light::White.value()])
                    .unwrap();
            } else {
                self.launchpad_output
                    .send(&[176, *id as u8, Light::RedLow.value()])
                    .unwrap();
            }
        }
    }

    fn refresh_side_buttons(&mut self) {
        let pos = self.last_pos;

        let beat_display_multiplier = (24.0 * 8.0) / self.loop_length.ticks() as f64;
        let shifted_beat_position = (pos.ticks() as f64 * beat_display_multiplier / 24.0) as usize;

        let current_beat_light = RIGHT_SIDE_BUTTONS[shifted_beat_position % 8];
        let current_repeat_light = RIGHT_SIDE_BUTTONS[REPEAT_RATES
            .iter()
            .position(|v| v == &self.rate)
            .unwrap_or(0)];

        let rate_color = if self.repeat_off_beat {
            Light::RedMed
        } else {
            Light::YellowMed
        };

        if current_repeat_light != self.last_repeat_light
            || self.last_repeat_light_out != rate_color
        {
            self.launchpad_output
                .send(&[144, self.last_repeat_light, 0])
                .unwrap();
            self.launchpad_output
                .send(&[144, current_repeat_light, rate_color.value()])
                .unwrap();
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

        let beat_light = if self.use_internal_clock.load(atomic::Ordering::Relaxed) {
            Light::Purple
        } else {
            Light::GreenLow
        };

        if current_beat_light != self.last_beat_light {
            self.launchpad_output
                .send(&[
                    144,
                    self.last_beat_light,
                    base_last_beat_light.unwrap_or(Light::Off).value(),
                ])
                .unwrap();
            if !beat_start {
                self.launchpad_output
                    .send(&[
                        144,
                        current_beat_light,
                        base_beat_light.unwrap_or(beat_light).value(),
                    ])
                    .unwrap();
            }
        }

        if beat_start {
            self.launchpad_output
                .send(&[144, current_beat_light, Light::White.value()])
                .unwrap();
        } else if pos.beat_tick() == 3 {
            self.launchpad_output
                .send(&[
                    144,
                    current_beat_light,
                    base_beat_light.unwrap_or(beat_light).value(),
                ])
                .unwrap();
        }

        self.last_beat_light = current_beat_light;
        self.last_repeat_light = current_repeat_light;
        self.last_repeat_light_out = rate_color;
    }

    fn refresh_undo_redo_lights(&mut self) {
        let color = if self.selecting_scale_held && self.shift_held {
            // nudging
            Light::Orange
        } else if self.shift_held {
            Light::GreenLow
        } else {
            Light::RedLow
        };

        self.launchpad_output
            .send(&[176, UNDO_BUTTON, color.value()])
            .unwrap();
        self.launchpad_output
            .send(&[176, REDO_BUTTON, color.value()])
            .unwrap();
    }

    fn refresh_loop_button(&mut self) {
        self.launchpad_output
            .send(&[176, LOOP_BUTTON, Light::YellowMed.value()])
            .unwrap();
    }

    fn refresh_loop_length(&mut self) {
        for (index, id) in LEFT_SIDE_BUTTONS.iter().enumerate() {
            let prev_button_length = *LOOP_LENGTHS
                .get(index.wrapping_sub(1))
                .unwrap_or(&MidiTime::zero());
            let button_length = LOOP_LENGTHS[index];
            let next_button_length = *LOOP_LENGTHS
                .get(index + 1)
                .unwrap_or(&(LOOP_LENGTHS[LOOP_LENGTHS.len() - 1] * 2));

            let result = if button_length == self.loop_length {
                Light::Yellow
            } else if self.loop_length < button_length && self.loop_length > prev_button_length {
                Light::Red
            } else if self.loop_length > button_length && self.loop_length < next_button_length {
                Light::Red
            } else {
                Light::Off
            };

            self.launchpad_output
                .send(&[176, *id, result.value()])
                .unwrap();
        }
    }

    fn toggle_internal_clock(&mut self) {
        let value = self
            .use_internal_clock
            .load(std::sync::atomic::Ordering::Relaxed);
        self.use_internal_clock
            .store(!value, std::sync::atomic::Ordering::Relaxed);
        self.refresh_selected_bank();
    }

    fn set_bank(&mut self, id: u8) {
        let mut params = self.params.lock().unwrap();
        params.bank = id;
    }

    fn grid_input(&mut self, id: u32, value: OutputValue) {
        let current_index = self.currently_held_inputs.iter().position(|v| v == &id);
        let scale_id = id + 64;
        let mut fresh_trigger = false;

        if value.is_on() {
            if current_index == None {
                self.currently_held_inputs.push(id);
                fresh_trigger = true;
            }
        } else if let Some(index) = current_index {
            self.currently_held_inputs.remove(index);
        }

        // use fresh_trigger detection to filter out aftertouch changes
        if self.shift_held && value.is_on() {
            if fresh_trigger {
                if self.selection.contains(&id) {
                    self.unselect(scale_id);
                    self.unselect(id);
                } else {
                    if self.selecting_scale {
                        // hack to avoid including drums/vox
                        self.select(scale_id);
                    } else {
                        self.select(id);
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
                            self.select(id);
                        }
                    }
                }
            }
        } else {
            // HACK: filter out aftertouch if that key wasn't already pressed (e.g. after releasing shift while still holding keys)
            let in_scale_view = id < 128
                && (self.selecting_scale
                    && (self.selection.len() == 0 || !self.selection.contains(&id)))
                || self.selection.contains(&scale_id);
            let (target_id, other_id) = if in_scale_view {
                (scale_id, id)
            } else {
                (id, scale_id)
            };

            if !value.is_on()
                || fresh_trigger
                || self
                    .input_values
                    .get(&target_id)
                    .unwrap_or(&OutputValue::Off)
                    .is_on()
            {
                self.input_values.insert(target_id, value);
                self.input_values.remove(&other_id);
                self.refresh_input(target_id);
                self.refresh_input(other_id);
            }
        }
        self.refresh_should_flatten();
    }

    fn refresh_all_inputs(&mut self) {
        for id in 0..136 {
            self.refresh_input(id);
        }
    }

    fn refresh_input(&mut self, id: u32) {
        let value = self.input_values.get(&id).unwrap_or(&OutputValue::Off);
        let original_value = self.override_values.get(&id).cloned();
        let transform = match value {
            &OutputValue::On(velocity) => {
                if let Some(mapped) = self.mapping.get(&Coords::from(id)) {
                    let offset = if self.repeat_off_beat {
                        self.rate / 2
                    } else {
                        MidiTime::zero()
                    };
                    match self
                        .chunk_repeat_mode
                        .get(&mapped.chunk_index)
                        .unwrap_or(&RepeatMode::Global)
                    {
                        RepeatMode::None | RepeatMode::OnlyQuant => {
                            LoopTransform::Value(OutputValue::On(velocity))
                        }
                        RepeatMode::NoCycle => match self.trigger_mode {
                            TriggerMode::Repeat | TriggerMode::Cycle => LoopTransform::Repeat {
                                rate: self.rate,
                                offset,
                                value: OutputValue::On(velocity),
                            },
                            _ => LoopTransform::Value(OutputValue::On(velocity)),
                        },
                        RepeatMode::Global => match self.trigger_mode {
                            TriggerMode::Repeat => LoopTransform::Repeat {
                                rate: self.rate,
                                offset,
                                value: OutputValue::On(velocity),
                            },
                            TriggerMode::Cycle => LoopTransform::Cycle {
                                rate: self.rate,
                                offset,
                                value: OutputValue::On(velocity),
                            },
                            _ => LoopTransform::Value(OutputValue::On(velocity)),
                        },
                    }
                } else {
                    LoopTransform::None
                }
            }
            &OutputValue::Off => LoopTransform::None,
        };

        let changed = match self.override_values.entry(id) {
            Occupied(mut entry) => {
                let different = entry.get() != &transform;
                entry.insert(transform.clone());
                different
            }
            Vacant(entry) => {
                let different = transform != LoopTransform::None;
                entry.insert(transform.clone());
                different
            }
        };

        if changed {
            match transform {
                LoopTransform::Repeat {
                    rate,
                    offset,
                    value,
                    ..
                } => {
                    if !matches!(original_value, Some(LoopTransform::Repeat { .. })) {
                        // we want to make sure this repeat does full gate cycle, calculate end time from current position
                        let to = next_repeat(self.last_pos + rate, rate, offset);
                        self.queue_repeat_trigger(id, transform.clone(), to)
                    } else if let Some(repeat_state) = self.repeat_states.get_mut(&id) {
                        // handle changing velocity
                        if repeat_state.phase == RepeatPhase::Pending
                            && matches!(repeat_state.transform, LoopTransform::Repeat { .. })
                        {
                            // wow, this is not good rust code :'(
                            if let LoopTransform::Repeat {
                                value: current_value,
                                ..
                            } = repeat_state.transform
                            {
                                if value > current_value {
                                    repeat_state.transform = transform
                                }
                            }
                        } else {
                            repeat_state.transform = transform
                        }
                    }
                }
                LoopTransform::Cycle {
                    rate,
                    offset,
                    value,
                    ..
                } => {
                    if !matches!(original_value, Some(LoopTransform::Cycle { .. })) {
                        // we want to make sure this repeat does full gate cycle, calculate end time from current position
                        let to = next_repeat(self.last_pos + rate, rate, offset);
                        self.queue_repeat_trigger(id, transform.clone(), to)
                    } else if let Some(repeat_state) = self.repeat_states.get_mut(&id) {
                        // handle changing velocity
                        if repeat_state.phase == RepeatPhase::Pending
                            && matches!(repeat_state.transform, LoopTransform::Cycle { .. })
                        {
                            // wow, this is not good rust code :'(
                            if let LoopTransform::Cycle {
                                value: current_value,
                                ..
                            } = repeat_state.transform
                            {
                                if value > current_value {
                                    repeat_state.transform = transform
                                }
                            }
                        } else {
                            repeat_state.transform = transform
                        }
                    }
                }
                LoopTransform::Value { .. } => {
                    let repeat_mode = if let Some(chunk_index) = self.chunk_index_for_id(id) {
                        self.chunk_repeat_mode
                            .get(&chunk_index)
                            .unwrap_or(&RepeatMode::Global)
                    } else {
                        &RepeatMode::Global
                    };

                    if self.trigger_mode == TriggerMode::Quantized
                        || (repeat_mode == &RepeatMode::OnlyQuant
                            && self.trigger_mode != TriggerMode::Immediate)
                    {
                        if !matches!(original_value, Some(LoopTransform::Value { .. })) {
                            // we want to make sure this repeat does full gate cycle, calculate end time from current position
                            let offset = if self.repeat_off_beat {
                                self.rate / 2
                            } else {
                                MidiTime::zero()
                            };
                            let to = next_repeat(self.last_pos, self.rate, offset);
                            self.queue_quantized_trigger(id, transform.clone(), to);
                        }
                    }
                }
                _ => (),
            }

            if get_schedule_mode(id, &self.chunks, &self.mapping) == ScheduleMode::Monophonic {
                // refresh all in this chunk if monophonic
                for id in get_all_ids_in_this_chunk(id, &self.mapping, &self.chunk_trigger_ids) {
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

    fn queue_repeat_trigger(&mut self, id: u32, transform: LoopTransform, to: MidiTime) {
        self.repeat_states.insert(
            id,
            RepeatState {
                phase: RepeatPhase::Pending,
                transform,
                to,
            },
        );
    }

    fn queue_quantized_trigger(&mut self, id: u32, transform: LoopTransform, to: MidiTime) {
        self.repeat_states.insert(
            id,
            RepeatState {
                phase: RepeatPhase::QuantizePending,
                transform,
                to,
            },
        );
    }

    fn refresh_override(&mut self, id: u32) {
        // use frozen loop if present

        let loop_collection = self.loop_state.get();

        let selection_override_loop_collection =
            if let Some(offset) = self.selection_override_offset {
                self.loop_state.retrieve(offset)
            } else {
                None
            };

        let mut transform =
            self.get_transform(id, &loop_collection, selection_override_loop_collection);

        // suppress if there are inputs held and monophonic scheduling
        if get_schedule_mode(id, &self.chunks, &self.mapping) == ScheduleMode::Monophonic
            && transform.is_active()
        {
            if !self
                .override_values
                .get(&id)
                .unwrap_or(&LoopTransform::None)
                .is_active()
            {
                // now check to see if any other triggers in the chunk have overrides
                let ids = get_all_ids_in_this_chunk(id, &self.mapping, &self.chunk_trigger_ids);
                let chunk_has_override = ids.iter().any(|id| {
                    self.override_values
                        .get(id)
                        .unwrap_or(&LoopTransform::None)
                        .is_active()
                });
                if chunk_has_override {
                    // suppress this override
                    transform = LoopTransform::Value(OutputValue::Off);
                }
            }
        }

        // if this note is part of selection, and other notes in selection are being overridden, then suppress this trigger
        let selection_active = self.selection.iter().any(|x| {
            self.override_values
                .get(x)
                .unwrap_or(&LoopTransform::None)
                .is_active()
        });
        if transform.is_active()
            && !self
                .override_values
                .get(&id)
                .unwrap_or(&LoopTransform::None)
                .is_active()
            && self.selection.contains(&id)
            && selection_active
        {
            transform = LoopTransform::Value(OutputValue::Off);
        }

        let last_value = self
            .out_transforms
            .get(&id)
            .unwrap_or(&LoopTransform::None)
            .unwrap_or(&LoopTransform::Value(OutputValue::Off));

        if last_value != transform.unwrap_or(&LoopTransform::Value(OutputValue::Off)) {
            let last_transform = self.out_transforms.get(&id).cloned();

            // mark all cycles as changed if one changes
            if matches!(transform, LoopTransform::Cycle { .. })
                || matches!(last_transform, Some(LoopTransform::Cycle { .. }))
            {
                self.mark_cycle_group_changed(id);
            }

            self.last_changed_triggers.insert(id, self.last_pos);
            self.out_transforms.insert(id, transform);

            // send new value
            if let Some(value) = self.get_value(id, self.last_pos, last_transform) {
                self.event(LoopEvent {
                    id,
                    value,
                    pos: self.last_pos,
                });
            }

            self.refresh_cycle_group_for(id);
        }
    }

    fn refresh_cycle_groups(&mut self) {
        for id in 0..136 {
            if let Some(chunk_index) = self.chunk_index_for_id(id) {
                let repeat_mode = self
                    .chunk_repeat_mode
                    .get(&chunk_index)
                    .unwrap_or(&RepeatMode::None);
                if repeat_mode == &RepeatMode::Global {
                    self.refresh_cycle_group_for(id);
                }
            }
        }
    }

    fn mark_cycle_group_changed(&mut self, id: u32) {
        if let Some(chunk_index) = self.chunk_index_for_id(id) {
            if let Some(steps) = self.cycle_groups.get(&chunk_index) {
                for step in steps {
                    self.last_changed_triggers.insert(step.id, self.last_pos);
                }
            }
        }
    }

    fn refresh_bottom_button(&mut self, base_id: u32) {
        let id = base_id + 128;

        let mapped = self.mapping.get(&Coords::from(id));
        let chunk_triggering_override = if let Some(mapped) = mapped {
            let chunk = &self.chunks[mapped.chunk_index];
            chunk.check_triggering(mapped.id)
        } else {
            None
        };

        let loop_triggering = self
            .out_values
            .get(&id)
            .unwrap_or(&OutputValue::Off)
            .is_on();

        let triggering = match chunk_triggering_override {
            None => loop_triggering,
            Some(value) => value,
        };

        let old_value = self.bottom_button_out.remove(&base_id);

        let color = if let Some(mapped) = mapped {
            let chunk = &self.chunks[mapped.chunk_index];
            if chunk.check_lit(mapped.id) {
                self.chunk_colors[mapped.chunk_index]
            } else {
                Light::Off
            }
        } else {
            Light::Off
        };

        let selection_color = Light::Green;

        let new_value = if triggering && self.selection.contains(&id) {
            LaunchpadLight::Pulsing(Light::White)
        } else if triggering {
            let trigger_color = if color == Light::Off {
                Light::Value(1)
            } else {
                Light::Value(3)
            };

            if self.active.contains(&id)
                || loop_triggering && chunk_triggering_override == Some(true)
            {
                LaunchpadLight::Pulsing(trigger_color)
            } else {
                LaunchpadLight::Constant(trigger_color)
            }
        } else if self.selection.contains(&id) {
            LaunchpadLight::Pulsing(selection_color)
        } else if self.recording.contains(&id) {
            LaunchpadLight::Pulsing(Light::RedLow)
        } else if self.active.contains(&id) {
            LaunchpadLight::Pulsing(color)
        } else if self.loop_state.is_frozen() {
            LaunchpadLight::Pulsing(Light::Orange)
        } else {
            LaunchpadLight::Constant(color)
        };

        if Some(new_value.clone()) != old_value {
            let midi_id = BOTTOM_BUTTONS.get(base_id as usize);
            let message = match new_value {
                LaunchpadLight::Constant(value) => [144, *midi_id.unwrap(), value.value()],
                LaunchpadLight::Pulsing(value) => [146, *midi_id.unwrap(), value.value()],
            };
            self.launchpad_output.send(&message).unwrap();
        }

        self.bottom_button_out.insert(base_id, new_value);
    }

    fn refresh_grid_button(&mut self, id: u32) {
        if id >= 128 {
            return self.refresh_bottom_button(id - 128);
        }

        let base_id = id % 64;

        let in_scale_view = (self.selecting_scale
            && (self.selection.len() == 0 || !self.selection.contains(&id)))
            || (self.shift_held && self.selecting_scale_held)
            || self.selection.contains(&(base_id + 64));

        let (id, background_id) = if in_scale_view {
            (base_id + 64, base_id)
        } else {
            (base_id, base_id + 64)
        };

        let mapped = self.mapping.get(&Coords::from(id));
        let background_mapped = self.mapping.get(&Coords::from(background_id));

        let chunk_triggering_override = if let Some(mapped) = mapped {
            let chunk = &self.chunks[mapped.chunk_index];
            chunk.check_triggering(mapped.id)
        } else {
            None
        };

        let loop_triggering = self
            .out_values
            .get(&id)
            .unwrap_or(&OutputValue::Off)
            .is_on();

        let triggering = match chunk_triggering_override {
            None => loop_triggering,
            Some(value) => value,
        };

        let background_triggering = if self
            .out_values
            .get(&background_id)
            .unwrap_or(&OutputValue::Off)
            .is_on()
        {
            true
        } else {
            false
        };

        let old_value = self.grid_out.remove(&base_id);

        let color = if let Some(mapped) = mapped {
            let chunk = &self.chunks[mapped.chunk_index];
            if chunk.check_lit(mapped.id) {
                self.chunk_colors[mapped.chunk_index]
            } else {
                Light::Off
            }
        } else {
            Light::Off
        };

        let selection_color = if in_scale_view {
            Light::Purple
        } else {
            if color == Light::Off {
                Light::GreenDark
            } else {
                Light::Green
            }
        };

        let background_color = if let Some(background_mapped) = background_mapped {
            self.chunk_colors[background_mapped.chunk_index]
        } else {
            Light::Off
        };

        let new_value = if triggering && self.selection.contains(&id) {
            LaunchpadLight::Pulsing(Light::Value(28))
        } else if triggering {
            let trigger_color = if color == Light::Off {
                Light::Value(1)
            } else {
                Light::Value(3)
            };

            if self.active.contains(&id)
                || loop_triggering && chunk_triggering_override == Some(true)
            {
                LaunchpadLight::Pulsing(trigger_color)
            } else {
                LaunchpadLight::Constant(trigger_color)
            }
        } else if self.selection.contains(&id) {
            LaunchpadLight::Pulsing(selection_color)
        } else if self.recording.contains(&id) {
            if color == Light::Off {
                LaunchpadLight::Pulsing(Light::Value(7))
            } else {
                LaunchpadLight::Pulsing(Light::Value(5))
            }
        } else if background_triggering {
            LaunchpadLight::Constant(background_color)
        } else if self.active.contains(&id) {
            LaunchpadLight::Pulsing(color)
        } else {
            LaunchpadLight::Constant(color)
        };

        if Some(new_value.clone()) != old_value {
            let midi_id = self.id_to_midi.get(&base_id);
            let message = match new_value {
                LaunchpadLight::Constant(value) => [144, *midi_id.unwrap(), value.value()],
                LaunchpadLight::Pulsing(value) => [146, *midi_id.unwrap(), value.value()],
            };
            self.launchpad_output.send(&message).unwrap();
        }

        self.grid_out.insert(base_id, new_value);
    }

    fn refresh_selection_override(&mut self) {
        self.selection_override = if self.suppressing {
            LoopTransform::Value(OutputValue::Off)
        } else if self.holding {
            LoopTransform::Range {
                pos: self.holding_at,
                length: self.rate,
            }
        } else {
            LoopTransform::None
        };

        for id in 0..136 {
            self.refresh_override(id);
        }
    }

    fn refresh_active(&mut self) {
        let current_loop = self.loop_state.get();
        let selection_override_loop_collection =
            if let Some(offset) = self.selection_override_offset {
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
                if is_active(
                    override_loop
                        .transforms
                        .get(id)
                        .unwrap_or(&LoopTransform::None),
                    id,
                    &self.recorder,
                ) {
                    ids.insert(*id);
                } else {
                    ids.remove(id);
                }
            }
        }

        let (added, removed) = update_ids(&ids, &mut self.active);
    }

    fn refresh_recording(&mut self) {
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
    }

    fn refresh_select_state(&mut self) {
        let new_state = if self.shift_held {
            Light::Green
        } else if self.selection.len() > 0 {
            Light::GreenLow
        } else {
            Light::Off
        };

        if self.select_out != new_state {
            self.launchpad_output
                .send(&[178, SHIFT_BUTTON, new_state.value()])
                .unwrap();
            self.select_out = new_state;
        }
    }

    fn event(&mut self, event: LoopEvent) {
        if let Some(mapped) = self.mapping.get(&Coords::from(event.id)).copied() {
            let new_value = event.value.clone();
            // if new_value.is_on() && new_value.value() < 25 {
            //     // reject less than 10 velocity
            //     return
            // }
            match maybe_update(&mut self.out_values, event.id, new_value) {
                Some(_) => {
                    self.trigger_chunk(mapped, new_value);

                    self.handle_repeat_trigger(event.id, new_value);
                }
                None => (),
            };

            self.recorder.add(event);

            // ensuring that repeat state completes a single cycle even if button is released early
        }
    }

    fn handle_repeat_trigger(&mut self, id: u32, value: OutputValue) {
        if let Some(repeat_state) = self.repeat_states.get_mut(&id) {
            if value.is_on() && repeat_state.phase == RepeatPhase::Pending {
                repeat_state.phase = RepeatPhase::Current;
            } else if !value.is_on() && repeat_state.phase == RepeatPhase::Current {
                repeat_state.phase = RepeatPhase::Triggered;
            }
        }
    }

    fn clear_recording(&mut self) {
        self.last_changed_triggers.clear();
    }

    fn tap_tempo(&mut self) {
        // TODO: make work
        // clock_sender.send(ToClock::TapTempo).unwrap();
    }

    fn start_loop(&mut self) {
        self.commit_selection_override();
        self.loop_held = true;
        self.loop_from = self.last_pos.round();
        self.launchpad_output
            .send(&[176, LOOP_BUTTON, Light::Green.value()])
            .unwrap();
    }

    fn end_loop(&mut self) {
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

        // make sure we include any currently held triggers
        for (id, value) in &self.input_values {
            if value.is_on() {
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

        for id in 0..136 {
            // include ids that are recording, or if self.shift_held, all active IDs!
            let selected = self.shift_held || self.selection.contains(&id);
            if recording_ids.contains(&id) || (selected && self.active.contains(&id)) {
                // only include in loop if there are items in the range
                let current_event = self.recorder.get_event_at(id, self.loop_from);
                let has_events =
                    self.recorder
                        .has_events(id, self.loop_from, self.loop_from + self.loop_length);
                if has_events {
                    new_loop.transforms.insert(
                        id,
                        LoopTransform::Range {
                            pos: self.loop_from,
                            length: self.loop_length,
                        },
                    );
                } else if let Some(current_event) = current_event {
                    // loop contains a single on note
                    new_loop
                        .transforms
                        .insert(id, LoopTransform::Value(current_event.value));
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

    fn select(&mut self, id: u32) {
        self.selection.insert(id);
        if let Some(mapped) = self.mapping.get(&Coords::from(id)) {
            let chunk = &mut self.chunks[mapped.chunk_index];
            chunk.select(mapped.id, true)
        }
    }

    fn unselect(&mut self, id: u32) {
        self.selection.remove(&id);
        if let Some(mapped) = self.mapping.get(&Coords::from(id)) {
            let chunk = &mut self.chunks[mapped.chunk_index];
            chunk.select(mapped.id, false)
        }
    }

    fn clear_selection(&mut self) {
        self.commit_selection_override();

        if !self.selecting_scale {
            self.selecting_scale = false;
            self.refresh_selecting_scale();
        }

        for id in &self.selection {
            if let Some(mapped) = self.mapping.get(&Coords::from(*id)) {
                let chunk = &mut self.chunks[mapped.chunk_index];
                chunk.select(mapped.id, false)
            }
        }

        self.selection.clear();

        self.refresh_select_state();
        self.refresh_selection_override();
    }

    fn refresh_should_flatten(&mut self) {
        let loop_collection = self.loop_state.get();
        let is_sustained = self.sustained_values.iter().any(|(key, value)| {
            value
                != loop_collection
                    .transforms
                    .get(key)
                    .unwrap_or(&LoopTransform::None)
        });
        let is_overridden = self
            .override_values
            .values()
            .any(|value| value != &LoopTransform::None);
        let new_value =
            &self.selection_override != &LoopTransform::None || is_overridden || is_sustained;
        if new_value != self.should_flatten {
            self.should_flatten = new_value;
            let color = if self.should_flatten {
                Light::GreenLow
            } else {
                Light::Off
            };
            self.launchpad_output
                .send(&[176, FLATTEN_BUTTON, color.value()])
                .unwrap();
        }
    }

    fn flatten(&mut self) {
        let mut new_loop = self.loop_state.get().clone();

        for id in 0..136 {
            let transform = self
                .out_transforms
                .get(&id)
                .unwrap_or(&LoopTransform::None)
                .clone();

            // check if there are actually events available for this range
            let is_empty = if let LoopTransform::Range { pos, length } = transform {
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

    fn clear_loops(&mut self, target: TransformTarget, clear_permanent: bool) {
        let mut new_loop = self.loop_state.get().clone();

        let ids: Vec<u32> = match target {
            TransformTarget::All => (0..136).collect(),
            TransformTarget::Main => (0..64).collect(),
            TransformTarget::Scale => (64..136).collect(),
            TransformTarget::Selected => self.selection.iter().cloned().collect(),
        };

        for id in ids {
            if clear_permanent || !self.no_suppress.contains(&id) {
                new_loop
                    .transforms
                    .insert(id, LoopTransform::Value(OutputValue::Off));
            }
        }

        self.loop_state.set(new_loop);
    }

    fn clear_automation(&mut self) {
        let mut params = self.params.lock().unwrap();
        params.reset_automation = true;
    }

    fn double_loop_length(&mut self) {
        self.set_loop_length(
            get_double_loop_length(self.loop_length).min(MidiTime::from_beats(32)),
        );
    }

    fn halve_loop_length(&mut self) {
        self.set_loop_length(
            get_half_loop_length(self.loop_length).max(MidiTime::from_measure(1, 4)),
        );
    }

    fn undo_selection(&mut self) {
        if let Some(next_offset) = self
            .loop_state
            .previous_index_for(self.selection_override_offset.unwrap_or(0), &self.selection)
        {
            self.selection_override_offset = Some(next_offset);
            self.refresh_selection_override();
        }
    }

    fn redo_selection(&mut self) {
        if let Some(next_offset) = self
            .loop_state
            .next_index_for(self.selection_override_offset.unwrap_or(0), &self.selection)
        {
            self.selection_override_offset = Some(next_offset);
            self.refresh_selection_override();
        }
    }

    fn set_loop_length(&mut self, loop_length: MidiTime) {
        self.loop_length = loop_length;
        self.refresh_loop_length();
    }

    fn freeze_button(&mut self, pressed: bool) {
        // send frozen to twister
        if pressed {
            self.loop_state.freeze();

            for (id, value) in &self.override_values {
                if value != &LoopTransform::None {
                    self.sustained_values.insert(*id, value.clone());
                }
            }
        } else {
            let frozen_state = self.loop_state.get().clone();
            self.loop_state.unfreeze();
            self.sustained_values.clear();

            // preserve any changes made that are currently selected
            let mut new_state = self.loop_state.get().clone();
            let mut updated = false;
            for id in &self.selection {
                let value = frozen_state.transforms.get(&id);
                if value != new_state.transforms.get(&id) {
                    if let Some(value) = value {
                        new_state.transforms.insert(*id, value.clone());
                        updated = true;
                    }
                }
            }
            if updated {
                self.loop_state.set(new_state);
            }
        }

        for id in 0..136 {
            self.refresh_override(id);
        }

        for id in 0..8 {
            self.refresh_bottom_button(id);
        }

        self.refresh_should_flatten();

        let mut params = self.params.lock().unwrap();
        params.frozen = pressed;
        params.cueing = false;
    }

    fn refresh_selecting_scale(&mut self) {
        if self.selecting_scale {
            self.launchpad_output
                .send(&[178, SESSION_BUTTON, Light::Yellow.value()])
                .unwrap();
        } else {
            self.launchpad_output
                .send(&[176, SESSION_BUTTON, 45])
                .unwrap();
        };
    }

    fn set_rate(&mut self, value: MidiTime) {
        self.rate = value;
        self.refresh_side_buttons();
        self.refresh_selection_override();
        self.refresh_override_repeat();
    }

    fn set_trigger_mode(&mut self, value: TriggerMode) {
        self.trigger_mode = value;
        self.refresh_override_repeat();
        self.refresh_selected_trigger_mode();
        self.refresh_all_inputs();
    }

    fn refresh_override_repeat(&mut self) {
        let mut to_update = HashMap::new();
        let mut to_refresh = HashSet::new();

        for (id, value) in &self.override_values {
            if let Some(_) = self.mapping.get(&Coords::from(*id)) {
                if let &LoopTransform::Repeat { offset, value, .. } = value {
                    to_update.insert(
                        *id,
                        LoopTransform::Repeat {
                            rate: self.rate,
                            offset,
                            value,
                        },
                    );
                } else if let &LoopTransform::Cycle { offset, value, .. } = value {
                    to_update.insert(
                        *id,
                        LoopTransform::Cycle {
                            rate: self.rate,
                            offset,
                            value,
                        },
                    );
                }
            }
        }

        for (id, value) in to_update {
            if let Some(repeat_state) = self.repeat_states.get_mut(&id) {
                repeat_state.transform = value.clone();
            }
            self.override_values.insert(id, value);
            to_refresh.insert(id);
        }

        let mut to_update_sustained = HashMap::new();

        for (id, transform) in &self.sustained_values {
            if let &LoopTransform::Cycle { offset, value, .. } = transform {
                to_update_sustained.insert(
                    *id,
                    LoopTransform::Cycle {
                        rate: self.rate,
                        offset,
                        value,
                    },
                );
            }
        }

        for (id, value) in to_update_sustained {
            self.sustained_values.insert(id, value);
            to_refresh.insert(id);
        }

        for id in to_refresh {
            self.refresh_override(id);
        }
    }

    fn initial_loop(&mut self) {
        for id in 0..136 {
            let loop_collection = self.loop_state.get();

            let selection_override_loop_collection =
                if let Some(offset) = self.selection_override_offset {
                    self.loop_state.retrieve(offset)
                } else {
                    None
                };
            let transform =
                self.get_transform(id, &loop_collection, selection_override_loop_collection);

            if self.out_transforms.get(&id).unwrap_or(&LoopTransform::None) != &transform {
                self.out_transforms.insert(id, transform);
                self.last_changed_triggers.insert(id, self.last_pos);

                // send new value
                if let Some(value) = self.get_value(id, self.last_pos, None) {
                    self.event(LoopEvent {
                        id: id,
                        value,
                        pos: self.last_pos,
                    });
                }
            }
        }
    }

    fn trigger_chunk(&mut self, map: MidiMap, value: OutputValue) {
        if let Some(chunk) = self.chunks.get_mut(map.chunk_index) {
            chunk.trigger(map.id, value);
            if value.is_on() {
                if let Some(channel) = self.chunk_channels.get(&map.chunk_index) {
                    let mut params = self.params.lock().unwrap();
                    params.channel_triggered.insert(*channel);
                }
            }
        }
    }

    fn chunk_tick(&mut self, chunk_index: usize) {
        if let Some(chunk) = self.chunks.get_mut(chunk_index) {
            chunk.on_tick(self.last_raw_pos);
        }
    }

    fn commit_selection_override(&mut self) {
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
                        self.last_changed_triggers
                            .insert(*id, self.last_pos.clone());
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

    fn chunk_index_for_id(&self, id: u32) -> Option<usize> {
        let map = self.mapping.get(&Coords::from(id));
        if let Some(MidiMap { chunk_index, .. }) = map {
            Some(*chunk_index)
        } else {
            None
        }
    }

    fn refresh_cycle_group_for(&mut self, id: u32) {
        if let Some(chunk_index) = self.chunk_index_for_id(id) {
            let transform = self.out_transforms.get(&id);
            let list: &mut Vec<CycleStep> =
                self.cycle_groups.entry(chunk_index).or_insert(Vec::new());

            if let Some(LoopTransform::Cycle {
                rate,
                offset,
                value,
            }) = transform.cloned()
            {
                // make sure it is in cycle group
                let step = CycleStep {
                    id,
                    rate,
                    offset,
                    value,
                };
                // insert step in ID order
                match list.binary_search_by(|v| v.id.cmp(&step.id)) {
                    // ID already exists, replace with new step
                    Ok(index) => {
                        list.push(step);
                        list.swap_remove(index);
                    }

                    // insert step in order
                    Err(index) => list.insert(index, step),
                };
            } else {
                // remove it
                if let Ok(index) = list.binary_search_by(|v| v.id.cmp(&id)) {
                    list.remove(index);

                    if list.len() > 0 {
                        // replace current step with new item
                        if let Entry::Occupied(mut current_step) =
                            self.chunk_cycle_step.entry(chunk_index)
                        {
                            if current_step.get().id == id {
                                let replacement = list.get(index).unwrap_or(list.get(0).unwrap());
                                current_step.insert(*replacement);
                            }
                        }
                    }
                }

                // reset if no more in group
                if list.len() == 0 {
                    self.chunk_cycle_step.remove(&chunk_index);
                    self.chunk_cycle_next_pos.remove(&chunk_index);
                }
            }
        }
    }

    fn update_cycle_steps(&mut self) {
        // increment next step and init
        for (chunk_id, steps) in &self.cycle_groups {
            if let Some(first_step) = steps.get(0) {
                if let Some(next_pos) = self.chunk_cycle_next_pos.get_mut(&chunk_id) {
                    if &self.last_pos > next_pos {
                        // bump time and increment step
                        let current_step = self
                            .chunk_cycle_step
                            .get(chunk_id)
                            .unwrap_or(first_step)
                            .clone();
                        let current_pos = steps
                            .iter()
                            .position(|x| x.id == current_step.id)
                            .unwrap_or(steps.len() - 1);
                        let next_step = steps.get(current_pos + 1).unwrap_or(first_step).clone();

                        self.chunk_cycle_step.insert(*chunk_id, next_step);
                        let pos = next_repeat(self.last_pos, next_step.rate, next_step.offset);
                        *next_pos = pos;
                    }
                } else {
                    // init time and clear step
                    let last_step = steps.get(steps.len() - 1).unwrap();
                    let next_pos = next_repeat(self.last_pos, first_step.rate, first_step.offset);
                    self.chunk_cycle_step.insert(*chunk_id, *last_step);
                    self.chunk_cycle_next_pos.insert(*chunk_id, next_pos);
                }
            }
        }
    }

    fn get_events(&self) -> Vec<LoopEvent> {
        let mut result = Vec::new();
        let position = self.last_pos;
        let length = self.last_length;

        if length > MidiTime::zero() {
            for (id, transform) in &self.out_transforms {
                match transform {
                    &LoopTransform::Range {
                        pos: range_pos,
                        length: range_length,
                    } => {
                        let playback_offset = range_pos % range_length;
                        let playback_pos =
                            range_pos + ((position - playback_offset) % range_length);

                        if range_pos >= playback_pos && range_pos < (playback_pos + length) {
                            // insert start value
                            if let Some(value) = self.get_value(*id, range_pos, None) {
                                LoopEvent {
                                    id: *id,
                                    pos: position,
                                    value,
                                }
                                .insert_into(&mut result);
                            }
                        }

                        let offset = position - playback_pos;
                        if let Some(events) =
                            self.recorder
                                .get_range_for(*id, playback_pos, playback_pos + length)
                        {
                            for event in events {
                                event.with_pos(event.pos + offset).insert_into(&mut result);
                            }
                        }
                    }
                    &LoopTransform::Repeat {
                        rate: repeat_rate,
                        offset: repeat_offset,
                        value,
                    } => {
                        let next_on = next_repeat(position, repeat_rate, repeat_offset);
                        let next_off =
                            next_repeat(position, repeat_rate, repeat_offset + repeat_rate.half());
                        let to = position + length;

                        if next_on >= position && next_on < to {
                            LoopEvent {
                                value,
                                pos: next_on,
                                id: id.clone(),
                            }
                            .insert_into(&mut result);
                        }

                        if next_off >= position && next_off < to {
                            LoopEvent {
                                value: OutputValue::Off,
                                pos: next_off,
                                id: id.clone(),
                            }
                            .insert_into(&mut result);
                        }
                    }
                    &LoopTransform::Cycle {
                        rate: repeat_rate,
                        offset: repeat_offset,
                        value,
                    } => {
                        let next_on = next_repeat(position, repeat_rate, repeat_offset);
                        let next_off =
                            next_repeat(position, repeat_rate, repeat_offset + repeat_rate.half());
                        let to = position + length;

                        if next_off >= position && next_off < to {
                            LoopEvent {
                                value: OutputValue::Off,
                                pos: next_off,
                                id: id.clone(),
                            }
                            .insert_into(&mut result);
                        }

                        if next_on >= position && next_on < to {
                            // only append if is the current trigger for chunk
                            if let Some(chunk_id) = self.chunk_index_for_id(*id) {
                                if let Some(step) = self.chunk_cycle_step.get(&chunk_id) {
                                    if step.id == *id {
                                        LoopEvent {
                                            value,
                                            pos: next_on,
                                            id: id.clone(),
                                        }
                                        .insert_into(&mut result);
                                    }
                                }
                            }
                        }
                    }
                    _ => (),
                }
            }
        }

        result
    }

    fn get_transform(
        &self,
        id: u32,
        loop_collection: &LoopCollection,
        override_collection: Option<&LoopCollection>,
    ) -> LoopTransform {
        let mut result = LoopTransform::None;

        let collection = if self.selection.contains(&id) && override_collection.is_some() {
            override_collection.unwrap()
        } else {
            loop_collection
        };

        if let Some(ref transform) = collection.transforms.get(&id) {
            result = transform.apply(&result);
        }

        // we avoid override in these cases unless it is a targeted suppress (in which case it is honored)
        let avoid_suppress = (self.no_suppress_held.contains(&id)
            && matches!(result, LoopTransform::Value(..)))
            || self.no_suppress.contains(&id);

        let sustained_value = self.sustained_values.get(&id);

        // use the sustained value if override value is none
        // what a mess!
        if let Some(value) = self.override_values.get(&id) {
            result = if value == &LoopTransform::None {
                sustained_value
                    .unwrap_or(&LoopTransform::None)
                    .apply(&result)
            } else {
                value.apply(&result)
            }
        } else if let Some(sustained_value) = sustained_value {
            result = sustained_value.apply(&result);
        }

        if ((self.selection.len() == 0 && !avoid_suppress) || self.selection.contains(&id))
            && result.is_active()
        {
            result = self.selection_override.apply(&result);
        }

        // handle triggering of "early repeat"
        if let Some(repeat_state) = self.repeat_states.get(&id) {
            if repeat_state.phase == RepeatPhase::QuantizePending {
                result = LoopTransform::None
            } else if repeat_state.phase != RepeatPhase::None {
                result = repeat_state.transform.clone()
            }
        }

        result
    }

    fn get_value(
        &self,
        id: u32,
        position: MidiTime,
        compare_value: Option<LoopTransform>,
    ) -> Option<OutputValue> {
        match self.out_transforms.get(&id).unwrap_or(&LoopTransform::None) {
            &LoopTransform::Value(value) => {
                if let Some(LoopTransform::Value(r_value)) = compare_value {
                    if value.is_on() == r_value.is_on() {
                        return None;
                    }
                }

                Some(value)
            }
            &LoopTransform::Range {
                pos: range_pos,
                length: range_length,
            } => {
                if let Some(LoopTransform::Range {
                    pos: r_pos,
                    length: r_length,
                }) = compare_value
                {
                    if r_pos == range_pos && r_length == range_length {
                        return None;
                    }
                }

                let playback_offset = range_pos % range_length;
                let playback_pos = range_pos + ((position - playback_offset) % range_length);
                match self.recorder.get_event_at(id, playback_pos) {
                    Some(event) if event.is_on() => {
                        match self.recorder.get_next_event_at(id, playback_pos) {
                            // don't force an output value if the next event is less than 1 beat away
                            Some(next_event)
                                if (next_event.pos - playback_pos) < MidiTime::from_beats(1) =>
                            {
                                None
                            }
                            _ => Some(event.value),
                        }
                    }
                    _ => Some(OutputValue::Off),
                }
            }
            &LoopTransform::Repeat { rate, offset, .. } => {
                if let Some(LoopTransform::Repeat {
                    rate: r_rate,
                    offset: r_offset,
                    ..
                }) = compare_value
                {
                    if r_rate == rate && r_offset == offset {
                        // don't override value if rate and offset are still the same -- it's due to pressure/aftertouch
                        return None;
                    }
                }

                Some(OutputValue::Off)
            }
            &LoopTransform::Cycle { rate, offset, .. } => {
                if let Some(LoopTransform::Cycle {
                    rate: r_rate,
                    offset: r_offset,
                    ..
                }) = compare_value
                {
                    if r_rate == rate && r_offset == offset {
                        // don't override value if rate and offset are still the same -- it's due to pressure/aftertouch
                        return None;
                    }
                }

                Some(OutputValue::Off)
            }
            _ => Some(OutputValue::Off),
        }
    }
}

fn maybe_update(
    hash_map: &mut HashMap<u32, OutputValue>,
    key: u32,
    new_value: OutputValue,
) -> Option<OutputValue> {
    let entry = hash_map.entry(key);
    match entry {
        Entry::Occupied(mut entry) => {
            let old_value = entry.insert(new_value);

            // only notify if the value has changed on state (not the specific value) to avoid double triggers with aftertouch
            if old_value.is_on() != new_value.is_on() {
                Some(new_value)
            } else {
                None
            }
        }
        Entry::Vacant(entry) => {
            entry.insert(new_value);
            Some(new_value)
        }
    }
}

fn get_grid_map() -> (HashMap<u8, u32>, HashMap<u32, u8>) {
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

fn update_ids<'a>(a: &'a HashSet<u32>, b: &'a mut HashSet<u32>) -> (Vec<u32>, Vec<u32>) {
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
    Pulsing(Light),
}

fn next_repeat(pos: MidiTime, rate: MidiTime, offset: MidiTime) -> MidiTime {
    let root = pos.quantize(rate) + (offset % rate);
    let result = if root < pos { root + rate } else { root };

    result
}

fn get_half_loop_length(time: MidiTime) -> MidiTime {
    let beats = time.as_float() / 24.0;
    let prev = prev_power_of_two((beats * 4.0) as u32) as f64 / 4.0;
    MidiTime::from_float(prev * 24.0)
}

fn get_double_loop_length(time: MidiTime) -> MidiTime {
    let beats = time.as_float() / 24.0;
    let next = next_power_of_two((beats * 4.0) as u32) as f64 / 4.0;
    MidiTime::from_float(next * 24.0)
}

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

fn get_schedule_mode(
    id: u32,
    chunks: &Vec<Box<dyn Triggerable>>,
    mapping: &HashMap<Coords, MidiMap>,
) -> ScheduleMode {
    if let Some(mapping) = mapping.get(&Coords::from(id)) {
        chunks.get(mapping.chunk_index).unwrap().schedule_mode()
    } else {
        ScheduleMode::MostRecent
    }
}

fn get_all_ids_in_this_chunk<'a>(
    id: u32,
    mapping: &HashMap<Coords, MidiMap>,
    chunk_trigger_ids: &'a Vec<Vec<u32>>,
) -> Vec<u32> {
    if let Some(mapping) = mapping.get(&Coords::from(id)) {
        chunk_trigger_ids.get(mapping.chunk_index).unwrap().clone()
    } else {
        Vec::new()
    }
}

fn is_active(transform: &LoopTransform, id: &u32, loop_recorder: &LoopRecorder) -> bool {
    match transform {
        LoopTransform::Range { pos, length } => {
            let has_events = loop_recorder.has_events(*id, *pos, *pos + *length);
            let has_start_value = if let Some(event) = loop_recorder.get_event_at(*id, *pos) {
                event.is_on()
            } else {
                true
            };

            has_events || has_start_value
        }
        _ => transform.has_sequence(),
    }
}

fn adjust_velocity(input_velocity: u8) -> u8 {
    if input_velocity < VELOCITY_THRESHOLD {
        0
    } else {
        let range = 127 - VELOCITY_THRESHOLD;
        let pos = input_velocity - VELOCITY_THRESHOLD;
        (pos as f64 / range as f64 * 127.0).min(127.0) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adjust_velocity() {
        assert_eq!(adjust_velocity(0), 0);
        assert_eq!(adjust_velocity(VELOCITY_THRESHOLD), 0);
        assert_eq!(adjust_velocity(VELOCITY_THRESHOLD + 1), 1);
        assert_eq!(adjust_velocity(VELOCITY_THRESHOLD + 2), 2);
        assert_eq!(adjust_velocity(64), 52);
        assert_eq!(adjust_velocity(126), 125);
        assert_eq!(adjust_velocity(127), 127);
    }
}
