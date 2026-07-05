extern crate circular_queue;
extern crate midir;

use self::circular_queue::CircularQueue;
use std::cmp::Ordering;
use std::collections::hash_map::Entry;
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::{Add, Sub};
use std::sync::atomic::{self, AtomicBool, Ordering as AtomicOrdering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const PLAYABLE_GRID_SIZE: u32 = 112;
const GRID_NOTE_START: u8 = 8;
const GRID_2_NOTE_START: u8 = 96;
const LENGTH_BUTTONS: [u8; 8] = [88, 89, 90, 91, 92, 93, 94, 95];
const BULK_DIGITAL_FEEDBACK_FRAME_REQUEST: u8 = 0x1D;
const BULK_DIGITAL_FEEDBACK_FRAME_FLAGS: u8 = 0x00;
const BULK_DIGITAL_FEEDBACK_MAX_RECORDS: usize = 32;
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

use midi_connection;
use midi_time::MidiTime;
use scheduler;

use chunk::{ChunkMap, Coords, LatchMode, MidiMap, RepeatMode, ScheduleMode, Triggerable};
use loop_recorder::{LoopEvent, LoopRecorder};
use loop_state::{LoopCollection, LoopState, LoopStateChange, LoopTransform};
use output_value::OutputValue;

const CONTROL_BUTTONS: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
const BANK_BUTTONS: [u8; 4] = [1, 2, 3, 4];
const BANK_COLORS: [u8; 4] = [0, 0, 0, 0];

const LOOP_BUTTON: u8 = CONTROL_BUTTONS[0];
const FLATTEN_BUTTON: u8 = CONTROL_BUTTONS[1];
const UNDO_BUTTON: u8 = CONTROL_BUTTONS[2];
const REDO_BUTTON: u8 = CONTROL_BUTTONS[3];
const SUPPRESS_BUTTON: u8 = CONTROL_BUTTONS[4];
const REPEAT_BUTTON: u8 = CONTROL_BUTTONS[5];
const PREPARE_BUTTON: u8 = CONTROL_BUTTONS[6];
const SELECT_BUTTON: u8 = CONTROL_BUTTONS[7];

lazy_static! {
    static ref REPEAT_RATES: [MidiTime; 8] = [
        MidiTime::from_measure(2, 1),
        MidiTime::from_measure(1, 1),
        MidiTime::from_measure(1, 2),
        MidiTime::from_measure(1, 4),
        MidiTime::from_measure(1, 8),
        MidiTime::from_measure(1, 6),
        MidiTime::from_measure(1, 3),
        MidiTime::from_measure(2, 3),
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
    pub select_held: bool,
    pub prepare_held: bool,
    pub root_overlay_until: Option<Instant>,
    pub root_overlay_note: Option<i32>,
    pub lfo_speed_overlay_until: Option<Instant>,
    pub lfo_speed_overlay_value: Option<String>,
    pub lfo_wave_overlay_until: Option<Instant>,
    pub lfo_wave_overlay_mode: Option<&'static str>,
    pub frozen: bool,
    pub cueing: bool,
    pub duck_triggered: bool,
    pub duck_tick_multiplier: f64,
    pub duck_reduction: f64,
    pub channel_triggered: HashSet<u32>,
    pub activity_flash_until: HashMap<u32, Instant>,
    pub reset_automation: bool,
    pub reset_beat: u32,
    pub active_notes: HashSet<u8>,
    pub slicer_offsets: HashMap<u32, HashMap<u32, u8>>,
    pub slicer_pitches: HashMap<u32, HashMap<u32, u8>>,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum TriggerMode {
    Immediate = 0,
    Quantized = 1,
    Repeat = 2,
    Cycle = 3,
}

struct LoopSnapshot {
    loop_from: MidiTime,
    loop_length: MidiTime,
    last_changed_triggers: HashMap<u32, MidiTime>,
    recording_ids: HashSet<u32>,
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

    fn override_mode(&self) -> TriggerMode {
        match self {
            TriggerMode::Repeat => TriggerMode::Quantized,
            _ => TriggerMode::Repeat,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum LoopGridRemoteEvent {
    DoubleButton(bool),
    LoopButton(bool),
    PrepareButton(bool),
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
    // controller palette values
    Value(u8),
    ValueLow(u8),
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
    WhiteMed,
    Off,
    None,
}

enum TransformTarget {
    All,
    Main,
    Selected,
}

#[derive(Clone, Copy)]
struct DigitalFeedback {
    color: u8,
    intensity: u8,
}

impl Light {
    pub fn unwrap_or(self, value: Light) -> Light {
        match self {
            Light::None => value,
            _ => self,
        }
    }

    pub fn low(&self) -> Self {
        match self {
            Light::Value(value) => Light::ValueLow(*value),
            _ => self.clone(),
        }
    }

    pub fn value(&self) -> u8 {
        match self {
            Light::Yellow => 22,
            Light::YellowMed => 22,
            Light::Lime => 37,
            Light::LimeLow => 40,
            Light::Purple => 97,
            Light::Green => 43,
            Light::GreenMed => 43,
            Light::GreenLow => 43,
            Light::GreenDark => 43,
            Light::Orange => 13,
            Light::OrangeMed => 13,
            Light::OrangeLow => 13,
            Light::Red => 1,
            Light::RedMed => 1,
            Light::RedLow => 1,
            Light::BlueDark => 85,
            Light::White => 127,
            Light::Value(value) => *value,
            _ => 0,
        }
    }
}

enum GridEvent {
    Connected,
    DelayedRefresh,

    LoopButton(bool),
    FlattenButton(bool),
    UndoButton(bool),
    RedoButton(bool),
    SuppressButton(bool),
    RepeatButton(bool),
    SelectButton(bool),
    PrepareButton(bool),

    None,
    LengthButton { id: usize, pressed: bool },
    RateButton { id: usize, pressed: bool },
    TriggerModeSelect { id: usize },
    SwingControl { value: u8 },
    TempoControl { value: u8 },
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

pub struct LoopGrid {
    pub remote_tx: mpsc::Sender<LoopGridRemoteEvent>,
    remote_queue: mpsc::Receiver<LoopGridRemoteEvent>,

    _input: midi_connection::ThreadReference,
    params: Arc<Mutex<LoopGridParams>>,
    use_internal_clock: Arc<AtomicBool>,
    internal_bpm: Arc<Mutex<f64>>,

    input_queue: mpsc::Receiver<GridEvent>,

    mapping: HashMap<Coords, MidiMap>,
    chunks: Vec<Box<dyn Triggerable>>,
    chunk_colors: Vec<Light>,
    chunk_channels: HashMap<usize, u32>,
    chunk_trigger_channels: HashMap<usize, Vec<u32>>,
    chunk_trigger_ids: Vec<Vec<u32>>,
    controller_output: midi_connection::SharedMidiOutputConnection,

    no_suppress: HashSet<u32>,
    no_suppress_held: HashSet<u32>,
    straight_timing_ids: HashSet<u32>,
    chunk_repeat_mode: HashMap<usize, RepeatMode>,
    loop_length: MidiTime,

    repeat_off_beat: bool,

    // selection
    selection_override: LoopTransform,
    selection: HashSet<u32>,
    suppressing: bool,
    holding: bool,
    holding_at: MidiTime,
    select_held: bool,
    selection_override_offset: Option<isize>,
    id_to_midi: HashMap<u32, u8>,
    pending_digital_feedback: HashMap<u16, DigitalFeedback>,
    digital_feedback_ready: Arc<AtomicBool>,

    loop_held: bool,
    loop_from: MidiTime,
    should_flatten: bool,

    last_flatten_press_at: Instant,
    delayed_refresh_at: Option<Instant>,
    delayed_refresh_done: bool,
    tempo_overlay_until: Option<Instant>,
    tempo_overlay_value: Option<u16>,
    control_overlay_until: Option<Instant>,
    control_overlay_label: Option<&'static str>,
    control_overlay_value: Option<String>,
    control_overlay_mode: Option<&'static str>,
    peek_root_overlay: bool,

    rate: MidiTime,
    recorder: LoopRecorder,

    last_pos: MidiTime,
    last_raw_pos: MidiTime,
    last_length: MidiTime,
    last_raw_length: MidiTime,

    current_bank: u8,

    sustained_values: HashMap<u32, LoopTransform>,
    override_values: HashMap<u32, LoopTransform>,
    input_values: HashMap<u32, OutputValue>,
    currently_held_inputs: Vec<u32>,
    last_changed_triggers: HashMap<u32, MidiTime>,
    // out state
    current_swing: f64,
    out_transforms: HashMap<u32, LoopTransform>,
    repeat_states: HashMap<u32, RepeatState>,

    out_values: HashMap<u32, OutputValue>,
    grid_out: HashMap<u32, GridLight>,
    length_row_out: HashMap<u8, Light>,
    repeat_button_out: Light,
    loop_button_out: Light,
    prepare_button_out: Light,
    select_out: Light,
    last_triggered: HashMap<usize, CircularQueue<u32>>,

    trigger_mode: TriggerMode,
    trigger_override_held: bool,
    preserve_immediate_until_release: HashSet<u32>,
    chunk_cycle_step: HashMap<usize, CycleStep>,
    chunk_cycle_next_pos: HashMap<usize, MidiTime>,
    cycle_groups: HashMap<usize, Vec<CycleStep>>,

    // display state
    active: HashSet<u32>,
    recording: HashSet<u32>,

    loop_state: LoopState,
    last_loop_snapshot: Option<LoopSnapshot>,
}

impl LoopGrid {
    fn send_note_color(&mut self, channel: u8, note: u8, light: Light) {
        if let Some(index) = digital_index_for_note(channel, note) {
            self.pending_digital_feedback.insert(
                index,
                DigitalFeedback {
                    color: light_to_hue(light.clone()),
                    intensity: light_to_intensity(light),
                },
            );
        }
    }

    fn queue_all_digital_feedback_off(&mut self) {
        for index in 0..140 {
            self.pending_digital_feedback.insert(
                index,
                DigitalFeedback {
                    color: 0,
                    intensity: 0,
                },
            );
        }
    }

    fn flush_digital_feedback(&mut self) {
        if !self.digital_feedback_ready.load(AtomicOrdering::SeqCst)
            || self.pending_digital_feedback.is_empty()
        {
            return;
        }

        let mut records: Vec<(u16, DigitalFeedback)> =
            self.pending_digital_feedback.drain().collect();
        records.sort_by_key(|(index, _)| *index);

        for chunk in records.chunks(BULK_DIGITAL_FEEDBACK_MAX_RECORDS) {
            send_digital_feedback_frame(&mut self.controller_output, chunk);
        }
    }

    pub fn new(
        chunk_map: Vec<Box<ChunkMap>>,
        params: Arc<Mutex<LoopGridParams>>,
        use_internal_clock: Arc<AtomicBool>,
        internal_bpm: Arc<Mutex<f64>>,
    ) -> Self {
        let (midi_to_id, _id_to_midi) = get_grid_map();

        let (input_queue_tx, input_queue) = mpsc::channel();
        let (remote_tx, remote_queue) = mpsc::channel();

        let input_queue_connect_tx = input_queue_tx.clone();
        let digital_feedback_ready = Arc::new(AtomicBool::new(false));
        let digital_feedback_ready_on_input = digital_feedback_ready.clone();
        let bulk_feedback_ready_polling = Arc::new(AtomicBool::new(false));
        let bulk_feedback_ready_polling_on_input = bulk_feedback_ready_polling.clone();

        let input = midi_connection::get_input(
            midi_connection::YAELTEX_PORT_NAME,
            move |stamp, message| {
                if message == BULK_FEEDBACK_READY_RESPONSE.as_slice() {
                    bulk_feedback_ready_polling_on_input.store(false, AtomicOrdering::SeqCst);
                    if digital_feedback_ready_on_input.swap(true, AtomicOrdering::SeqCst) {
                        return;
                    }

                    println!("[INFO] Grid digital bulk feedback ready");
                    input_queue_connect_tx.send(GridEvent::Connected).unwrap();
                    return;
                }

                if message == [0xF0, 0x79, 0x74, 0x78, 0x01, 0xF7] {
                    return;
                }

                if message.is_empty() {
                    return;
                }

                let status = message[0];
                let channel = (status & 0x0F) + 1;
                let message_type = status & 0xF0;

                match message_type {
                    0x90 | 0x80 => {
                        let note = message[1];
                        let value = if message_type == 0x80 { 0 } else { message[2] };
                        let pressed = value > 0;

                        if channel == 1 {
                            if let Some(id) = midi_to_id.get(&note) {
                                input_queue_tx
                                    .send(GridEvent::GridInput {
                                        stamp,
                                        id: *id,
                                        value,
                                    })
                                    .unwrap();
                            } else if let Some(id) = CONTROL_BUTTONS.iter().position(|&x| x == note)
                            {
                                input_queue_tx
                                    .send(match id {
                                        0 => GridEvent::LoopButton(pressed),
                                        1 => GridEvent::FlattenButton(pressed),
                                        2 => GridEvent::UndoButton(pressed),
                                        3 => GridEvent::RedoButton(pressed),
                                        4 => GridEvent::SuppressButton(pressed),
                                        5 => GridEvent::RepeatButton(pressed),
                                        6 => GridEvent::PrepareButton(pressed),
                                        7 => GridEvent::SelectButton(pressed),
                                        _ => GridEvent::None,
                                    })
                                    .unwrap();
                            } else if let Some(id) = LENGTH_BUTTONS.iter().position(|&x| x == note)
                            {
                                input_queue_tx
                                    .send(GridEvent::LengthButton { id, pressed })
                                    .unwrap();
                            }
                        } else if channel == 2 {
                            if let Some(id) = BANK_BUTTONS.iter().position(|&x| x == note) {
                                input_queue_tx
                                    .send(GridEvent::BankButton { id, pressed })
                                    .unwrap();
                            }
                        }
                    }
                    0xB0 => {
                        if channel == 2 {
                            match message[1] {
                                1 => {
                                    let id = cc_bucket(message[2], REPEAT_RATES.len());
                                    input_queue_tx
                                        .send(GridEvent::RateButton { id, pressed: true })
                                        .unwrap();
                                }
                                2 => {
                                    let id = cc_bucket(message[2], 4);
                                    input_queue_tx
                                        .send(GridEvent::TriggerModeSelect { id })
                                        .unwrap();
                                }
                                3 => {
                                    input_queue_tx
                                        .send(GridEvent::SwingControl { value: message[2] })
                                        .unwrap();
                                }
                                4 => {
                                    input_queue_tx
                                        .send(GridEvent::TempoControl { value: message[2] })
                                        .unwrap();
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            },
        );

        let (_midi_to_id, id_to_midi) = get_grid_map();
        let loop_length = MidiTime::from_beats(8);
        let mut base_loop = LoopCollection::new(loop_length);

        let mut controller_output =
            midi_connection::get_shared_output(midi_connection::YAELTEX_PORT_NAME);
        let digital_feedback_ready_on_connect = digital_feedback_ready.clone();
        let bulk_feedback_ready_polling_on_connect = bulk_feedback_ready_polling.clone();
        let feedback_poll = controller_output.clone();
        controller_output.on_connect(move |_port| {
            digital_feedback_ready_on_connect.store(false, AtomicOrdering::SeqCst);
            if bulk_feedback_ready_polling_on_connect.swap(true, AtomicOrdering::SeqCst) {
                return;
            }

            let bulk_feedback_ready_polling = bulk_feedback_ready_polling_on_connect.clone();
            let mut feedback_poll = feedback_poll.clone();
            thread::spawn(move || {
                for _ in 0..BULK_FEEDBACK_READY_POLL_MAX_ATTEMPTS {
                    if !bulk_feedback_ready_polling.load(AtomicOrdering::SeqCst) {
                        return;
                    }
                    let _ = feedback_poll.send(&BULK_FEEDBACK_READY_POLL);
                    thread::sleep(Duration::from_millis(BULK_FEEDBACK_READY_POLL_INTERVAL_MS));
                }
                bulk_feedback_ready_polling.store(false, AtomicOrdering::SeqCst);
                eprintln!("[WARN] Timed out waiting for Yaeltex bulk feedback ready response");
            });
        });

        let mut instance = LoopGrid {
            _input: input,
            controller_output,
            loop_length,
            params,
            use_internal_clock,
            internal_bpm,
            id_to_midi,

            trigger_mode: TriggerMode::Immediate,
            trigger_override_held: false,
            preserve_immediate_until_release: HashSet::new(),
            pending_digital_feedback: HashMap::new(),
            digital_feedback_ready,
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
            chunk_trigger_channels: HashMap::new(),
            chunk_trigger_ids: Vec::new(),

            no_suppress: HashSet::new(),
            no_suppress_held: HashSet::new(),
            straight_timing_ids: HashSet::new(),

            repeat_off_beat: false,

            // selection
            selection_override: LoopTransform::None,
            selection: HashSet::new(),
            suppressing: false,
            holding: false,
            holding_at: MidiTime::zero(),
            select_held: false,
            selection_override_offset: None,

            loop_held: false,
            loop_from: MidiTime::from_ticks(0),
            should_flatten: false,
            last_flatten_press_at: Instant::now(),
            delayed_refresh_at: None,
            delayed_refresh_done: false,
            tempo_overlay_until: None,
            tempo_overlay_value: None,
            control_overlay_until: None,
            control_overlay_label: None,
            control_overlay_value: None,
            control_overlay_mode: None,
            peek_root_overlay: false,

            rate: MidiTime::from_beats(2),
            recorder: LoopRecorder::new(),

            last_pos: MidiTime::from_ticks(0),
            last_raw_pos: MidiTime::from_ticks(0),
            last_length: MidiTime::from_ticks(0),
            last_raw_length: MidiTime::from_ticks(0),

            current_bank: 0,

            sustained_values: HashMap::new(),
            override_values: HashMap::new(),
            input_values: HashMap::new(),
            currently_held_inputs: Vec::new(),
            last_changed_triggers: HashMap::new(),

            // out state
            current_swing: 0.0,
            out_transforms: HashMap::new(),
            repeat_states: HashMap::new(),

            out_values: HashMap::new(),
            grid_out: HashMap::new(),
            length_row_out: HashMap::new(),
            repeat_button_out: Light::Off,
            loop_button_out: Light::Off,
            prepare_button_out: Light::Off,
            select_out: Light::Off,
            last_triggered: HashMap::new(),

            // display state
            active: HashSet::new(),
            recording: HashSet::new(),

            loop_state: LoopState::new(loop_length),
            last_loop_snapshot: None,
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

            if item.repeat_mode == RepeatMode::OnlyQuant || item.no_suppress_held {
                for id in &trigger_ids {
                    instance.no_suppress_held.insert(*id);
                }
            }

            for local_id in &item.straight_timing_local_ids {
                if let Some(id) = trigger_ids.get(*local_id as usize) {
                    instance.straight_timing_ids.insert(*id);
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
            if let Some(trigger_channels) = item.trigger_channels {
                instance
                    .chunk_trigger_channels
                    .insert(chunk_index, trigger_channels);
            }

            instance.chunks.push(item.chunk);
        }

        // create base level undo
        instance.loop_state.set(base_loop);

        instance
    }

    fn refresh_grid_buttons(&mut self) {
        for id in 0..PLAYABLE_GRID_SIZE {
            self.refresh_grid_button(id);
        }
    }

    fn grid_input_event(&mut self, event: GridEvent) {
        match event {
            GridEvent::Connected => {
                println!("Controller Connected");
                self.queue_all_digital_feedback_off();
                self.grid_out.clear();
                self.length_row_out.clear();
                self.repeat_button_out = Light::Off;
                self.loop_button_out = Light::Off;
                self.prepare_button_out = Light::Off;
                self.delayed_refresh_at = Some(Instant::now() + Duration::from_millis(1000));
                self.delayed_refresh_done = false;
                self.refresh_grid_buttons();
                self.refresh_loop_button();
                self.refresh_undo_redo_lights();
                self.refresh_suppress_button();
                self.refresh_selected_bank();
                self.refresh_loop_length();
                self.refresh_repeat_button();
                self.refresh_prepare_button();
                self.refresh_select_state();
            }
            GridEvent::DelayedRefresh => {
                self.queue_all_digital_feedback_off();
                self.grid_out.clear();
                self.length_row_out.clear();
                self.repeat_button_out = Light::Off;
                self.loop_button_out = Light::Off;
                self.prepare_button_out = Light::Off;
                self.refresh_grid_buttons();
                self.refresh_loop_button();
                self.refresh_undo_redo_lights();
                self.refresh_suppress_button();
                self.refresh_selected_bank();
                self.refresh_loop_length();
                self.refresh_repeat_button();
                self.refresh_prepare_button();
                self.refresh_select_state();
            }
            GridEvent::LoopButton(pressed) => {
                if pressed {
                    self.start_loop();
                } else {
                    self.end_loop();
                }
            }
            GridEvent::FlattenButton(pressed) => {
                if pressed {
                    if self.last_flatten_press_at.elapsed() > Duration::from_millis(200) {
                        if self.trigger_override_held {
                            for id in &self.currently_held_inputs {
                                self.preserve_immediate_until_release.insert(*id);
                            }
                        }

                        self.commit_selection_override();
                        if self.should_flatten {
                            self.flatten();
                        } else if self.selection.len() > 0 {
                            self.clear_loops(TransformTarget::Selected, true);
                        } else {
                            if self.select_held {
                                self.clear_loops(TransformTarget::All, false);
                                self.clear_automation();
                            } else {
                                self.clear_loops(TransformTarget::Main, false);
                            }
                        }
                        self.clear_selection();
                    }

                    self.last_flatten_press_at = Instant::now();
                }
            }
            GridEvent::UndoButton(pressed) => {
                if pressed {
                    if self.select_held {
                        self.halve_loop_length();
                    } else if self.selection.len() > 0 {
                        self.undo_selection();
                    } else {
                        self.loop_state.undo();
                    }
                    self.clear_loop_snapshot();
                }
            }
            GridEvent::RedoButton(pressed) => {
                if pressed {
                    if self.select_held {
                        self.double_loop_length();
                    } else if self.selection.len() > 0 {
                        self.redo_selection();
                    } else {
                        self.loop_state.redo();
                    }
                    self.clear_loop_snapshot();
                }
            }
            GridEvent::SuppressButton(pressed) => {
                self.suppressing = pressed;
                self.refresh_suppress_button();
                self.refresh_selection_override();
                self.refresh_should_flatten();
            }
            GridEvent::RepeatButton(pressed) => {
                self.holding = pressed;
                self.holding_at = self.last_pos;
                self.refresh_repeat_button();
                self.refresh_selection_override();
                self.refresh_should_flatten();
            }
            GridEvent::SelectButton(pressed) => {
                self.select_held = pressed;
                self.params.lock().unwrap().select_held = pressed;
                if pressed {
                    self.clear_selection()
                } else {
                    self.peek_root_overlay = false;
                }

                self.refresh_select_state();
                self.refresh_undo_redo_lights();
            }
            GridEvent::LengthButton { id, pressed } => {
                if pressed {
                    let length = LOOP_LENGTHS[id % LOOP_LENGTHS.len()];
                    self.set_loop_length(length);
                    if self.select_held {
                        self.reloop(length)
                    }
                }
            }
            GridEvent::RateButton { id, pressed: _ } => {
                let rate = REPEAT_RATES[id as usize];
                self.repeat_off_beat = self.select_held;
                self.set_rate(rate);
            }
            GridEvent::TriggerModeSelect { id } => {
                self.set_trigger_mode(TriggerMode::from_id(id));
            }
            GridEvent::SwingControl { value } => {
                self.set_swing(value);
            }
            GridEvent::TempoControl { value } => {
                self.set_tempo(value);
            }
            GridEvent::BankButton { id, pressed } => {
                if self.select_held {
                    if pressed {
                        self.show_peek_overlay_for_bank(id);
                    }
                } else if pressed {
                    self.set_bank(id as u8);
                }
            }
            GridEvent::GridInput {
                id,
                value,
                stamp: _,
            } => {
                if value > 0 {
                    self.grid_input(id, OutputValue::On(value));
                } else {
                    self.grid_input(id, OutputValue::Off);
                }
            }
            GridEvent::PrepareButton(pressed) => {
                self.set_trigger_override_held(pressed);
            }
            GridEvent::None => (),
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
            LoopGridRemoteEvent::PrepareButton(pressed) => {
                self.set_trigger_override_held(pressed);
            }
        }
    }

    fn clear_loop_snapshot(&mut self) {
        self.last_loop_snapshot = None;
        self.refresh_loop_length();
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

        if let Some(at) = self.delayed_refresh_at {
            if !self.delayed_refresh_done && Instant::now() >= at {
                self.delayed_refresh_done = true;
                self.grid_input_event(GridEvent::DelayedRefresh);
            }
        }

        let grid_events: Vec<GridEvent> = self.input_queue.try_iter().collect();
        for event in grid_events {
            self.grid_input_event(event)
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
        self.last_raw_length = range.to - range.from;
        self.last_pos = range.from.swing(self.current_swing);
        self.last_length = range.to.swing(self.current_swing) - self.last_pos;

        if range.jumped {
            self.initial_loop();
        }

        if range.ticked {
            // handle revert of loop length button

            self.refresh_loop_length();
            self.refresh_recording();
            self.refresh_loop_button();
            self.refresh_repeat_button();
            self.refresh_prepare_button();
        }

        // consume controller and other controllers
        self.drain_input_events();

        // clear repeats from last cycle
        let mut to_refresh = Vec::new();

        for (id, repeat_state) in &mut self.repeat_states {
            let pos = if self.straight_timing_ids.contains(id) {
                self.last_raw_pos
            } else {
                self.last_pos
            };
            if repeat_state.phase != RepeatPhase::None && pos >= repeat_state.to {
                if let Some(LoopTransform::Repeat { rate, offset, .. }) =
                    self.override_values.get(&id)
                {
                    repeat_state.to =
                        next_repeat(pos + MidiTime::from_sub_ticks(1), *rate, *offset);
                    repeat_state.phase = RepeatPhase::Current;
                } else if let Some(LoopTransform::Cycle { rate, offset, .. }) =
                    self.override_values.get(&id)
                {
                    repeat_state.to =
                        next_repeat(pos + MidiTime::from_sub_ticks(1), *rate, *offset);
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
        self.refresh_grid_buttons();
        self.flush_digital_feedback();
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
        let internal_clock = self.use_internal_clock.load(atomic::Ordering::Relaxed);
        for (index, id) in BANK_BUTTONS.iter().enumerate() {
            let bank_light = if internal_clock {
                Light::Purple
            } else {
                Light::Value(BANK_COLORS[index])
            };
            if self.current_bank == index as u8 {
                self.send_note_color(2, *id as u8, Light::White);
            } else {
                self.send_note_color(2, *id as u8, bank_light);
            }
        }
    }

    fn refresh_undo_redo_lights(&mut self) {
        let color = if self.select_held {
            Light::GreenLow
        } else {
            Light::RedLow
        };

        self.send_note_color(1, UNDO_BUTTON, color);
        self.send_note_color(1, REDO_BUTTON, color);
    }

    fn refresh_loop_button(&mut self) {
        let light = if self.loop_held {
            Light::White
        } else if self.has_loopable_material() {
            Light::GreenLow
        } else {
            Light::Off
        };

        if self.loop_button_out != light {
            self.send_note_color(1, LOOP_BUTTON, light);
            self.loop_button_out = light;
        }
    }

    fn has_loopable_material(&self) -> bool {
        let from = self.last_pos - self.loop_length;

        self.last_changed_triggers
            .values()
            .any(|last_changed| last_changed >= &from)
            || self.input_values.values().any(|value| value.is_on())
            || self
                .override_values
                .values()
                .any(|value| value != &LoopTransform::None)
    }

    fn refresh_suppress_button(&mut self) {
        let light = if self.suppressing {
            Light::White
        } else {
            Light::Off
        };
        self.send_note_color(1, SUPPRESS_BUTTON, light);
    }

    fn refresh_prepare_button(&mut self) {
        let light =
            trigger_mode_button_light(self.effective_trigger_mode(), self.trigger_override_held);

        if self.prepare_button_out != light {
            self.send_note_color(1, PREPARE_BUTTON, light);
            self.prepare_button_out = light;
        }
    }

    fn refresh_repeat_button(&mut self) {
        let offset = if self.repeat_off_beat {
            self.rate / 2
        } else {
            MidiTime::zero()
        };
        let cycle_pos = (self.last_pos - offset) % self.rate;
        let on_color = if is_triplet_rate(self.rate) {
            Light::Purple
        } else {
            Light::Green
        };
        let light = if self.holding {
            Light::White
        } else if cycle_pos < self.rate.half() {
            on_color
        } else {
            Light::Off
        };

        if self.repeat_button_out != light {
            self.send_note_color(1, REPEAT_BUTTON, light);
            self.repeat_button_out = light;
        }
    }

    fn refresh_loop_length(&mut self) {
        let mut pending = Vec::new();

        if self.display_overlay_active() {
            for (col, id) in LENGTH_BUTTONS.iter().enumerate() {
                let light = self.display_overlay_light(0, col).unwrap_or(Light::Off);
                pending.push((*id, light));
            }
        } else {
            let pos = self.last_pos;
            let beat_display_multiplier = (24.0 * 8.0) / self.loop_length.ticks() as f64;
            let shifted_beat_position =
                (pos.ticks() as f64 * beat_display_multiplier / 24.0) as usize;
            let current_beat_index = shifted_beat_position % 8;
            let beat_start = pos % MidiTime::from_beats(1) < MidiTime::from_float(2.0);
            let selected_color = if self.repeat_off_beat {
                Light::RedMed
            } else {
                Light::YellowMed
            };

            for (index, id) in LENGTH_BUTTONS.iter().enumerate() {
                let prev_button_length = *LOOP_LENGTHS
                    .get(index.wrapping_sub(1))
                    .unwrap_or(&MidiTime::zero());
                let button_length = LOOP_LENGTHS[index];
                let next_button_length = *LOOP_LENGTHS
                    .get(index + 1)
                    .unwrap_or(&(LOOP_LENGTHS[LOOP_LENGTHS.len() - 1] * 2));

                let base = if button_length == self.loop_length {
                    selected_color
                } else if self.loop_length < button_length && self.loop_length > prev_button_length
                {
                    Light::Red
                } else if self.loop_length > button_length && self.loop_length < next_button_length
                {
                    Light::Red
                } else {
                    Light::Off
                };

                let result = if beat_start && index == current_beat_index {
                    Light::White
                } else if index == current_beat_index && base == Light::Off {
                    Light::GreenLow
                } else {
                    base
                };

                pending.push((*id, result));
            }
        }

        for (id, light) in pending {
            if self.length_row_out.get(&id) != Some(&light) {
                self.send_note_color(1, id, light);
                self.length_row_out.insert(id, light);
            }
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
        let mut fresh_trigger = false;

        if value.is_on() {
            if current_index == None {
                self.currently_held_inputs.push(id);
                fresh_trigger = true;
            }
        } else if let Some(index) = current_index {
            self.currently_held_inputs.remove(index);
            self.preserve_immediate_until_release.remove(&id);
        }

        if self.select_held && value.is_on() {
            if fresh_trigger {
                if self.selection.contains(&id) {
                    self.unselect(id);
                } else {
                    self.select(id);
                }

                if self.currently_held_inputs.len() == 2 {
                    let from = Coords::from(self.currently_held_inputs[0]);
                    let to = Coords::from(self.currently_held_inputs[1]);

                    let from_row = u32::min(from.row, to.row);
                    let to_row = u32::max(from.row, to.row) + 1;
                    let from_col = u32::min(from.col, to.col);
                    let to_col = u32::max(from.col, to.col) + 1;

                    for row in from_row..to_row {
                        for col in from_col..to_col {
                            self.select(Coords::id_from(row, col));
                        }
                    }
                }
            }
        } else if !value.is_on()
            || fresh_trigger
            || self
                .input_values
                .get(&id)
                .unwrap_or(&OutputValue::Off)
                .is_on()
        {
            self.input_values.insert(id, value);
            self.refresh_input(id);
        }
        self.refresh_loop_button();
        self.refresh_should_flatten();
    }

    fn refresh_all_inputs(&mut self) {
        for id in 0..PLAYABLE_GRID_SIZE {
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
                        RepeatMode::NoCycle => match self.effective_trigger_mode_for_id(id) {
                            TriggerMode::Repeat | TriggerMode::Cycle => LoopTransform::Repeat {
                                rate: self.rate,
                                offset,
                                value: OutputValue::On(velocity),
                            },
                            _ => LoopTransform::Value(OutputValue::On(velocity)),
                        },
                        RepeatMode::Global => match self.effective_trigger_mode_for_id(id) {
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
                        let pos = self.pos_for_id(id);
                        let to = next_repeat(pos + rate, rate, offset);
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
                        let pos = self.pos_for_id(id);
                        let to = next_repeat(pos + rate, rate, offset);
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

                    let trigger_mode = self.effective_trigger_mode_for_id(id);
                    let should_quantize = trigger_mode == TriggerMode::Quantized
                        || (repeat_mode == &RepeatMode::OnlyQuant
                            && trigger_mode != TriggerMode::Immediate);
                    if should_quantize {
                        if !matches!(original_value, Some(LoopTransform::Value { .. })) {
                            // we want to make sure this repeat does full gate cycle, calculate end time from current position
                            let offset = if self.repeat_off_beat {
                                self.rate / 2
                            } else {
                                MidiTime::zero()
                            };
                            let pos = self.pos_for_id(id);
                            let to = next_repeat(pos, self.rate, offset);
                            self.queue_quantized_trigger(id, transform.clone(), to);
                        }
                    } else {
                        // When a held input moves from repeat/cycle back to plain trigger
                        // because PREPARE was released, any existing repeat state must stop
                        // immediately or it will keep overriding get_transform().
                        self.repeat_states.remove(&id);
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

            let pos = self.pos_for_id(id);
            self.last_changed_triggers.insert(id, pos);
            self.out_transforms.insert(id, transform);

            // send new value
            if let Some(value) = self.get_value(id, pos, last_transform) {
                self.event(LoopEvent { id, value, pos });
            }

            self.refresh_cycle_group_for(id);
        }
    }

    fn refresh_cycle_groups(&mut self) {
        for id in 0..PLAYABLE_GRID_SIZE {
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
                    self.last_changed_triggers
                        .insert(step.id, self.pos_for_id(step.id));
                }
            }
        }
    }

    fn refresh_grid_button(&mut self, id: u32) {
        if let Some(light) = self.tempo_overlay_light(id) {
            let new_value = GridLight::Constant(light);
            let old_value = self.grid_out.remove(&id);

            if Some(new_value.clone()) != old_value {
                let midi_id = self.id_to_midi.get(&id).unwrap();
                match new_value.clone() {
                    GridLight::Constant(value) | GridLight::Pulsing(value) => {
                        self.send_note_color(1, *midi_id, value)
                    }
                }
            }

            self.grid_out.insert(id, new_value);
            return;
        }

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

        let old_value = self.grid_out.remove(&id);

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

        let selection_color = if color == Light::Off {
            Light::GreenDark
        } else {
            Light::Green
        };

        let new_value = if triggering && self.selection.contains(&id) {
            GridLight::Constant(Light::White)
        } else if triggering {
            let trigger_color = if color == Light::Off {
                Light::WhiteMed
            } else {
                Light::White
            };

            GridLight::Constant(trigger_color)
        } else if self.selection.contains(&id) {
            GridLight::Constant(selection_color)
        } else if self.recording.contains(&id) {
            if color == Light::Off {
                GridLight::Constant(Light::RedMed)
            } else {
                GridLight::Constant(Light::Red)
            }
        } else if self.active.contains(&id) {
            GridLight::Constant(color)
        } else if self.loop_state.is_frozen() {
            GridLight::Constant(Light::Orange)
        } else {
            GridLight::Constant(color)
        };

        if Some(new_value.clone()) != old_value {
            let midi_id = self.id_to_midi.get(&id).unwrap();
            match new_value.clone() {
                GridLight::Constant(value) | GridLight::Pulsing(value) => {
                    self.send_note_color(1, *midi_id, value)
                }
            }
        }

        self.grid_out.insert(id, new_value);
    }

    fn root_overlay_active(&self) -> bool {
        let params = self.params.lock().unwrap();
        if self.peek_root_overlay {
            return true;
        }
        if let Some(until) = params.root_overlay_until {
            Instant::now() <= until && params.root_overlay_note.is_some()
        } else {
            false
        }
    }

    fn display_overlay_active(&self) -> bool {
        self.root_overlay_active()
            || self.lfo_speed_overlay_active()
            || self.lfo_wave_overlay_active()
            || self.tempo_overlay_active()
            || self.control_overlay_active()
    }

    fn root_overlay_display_light(&self, row: usize, col: usize) -> Option<Light> {
        if !self.root_overlay_active() {
            return None;
        }

        let params = self.params.lock().unwrap();
        let note = params.root_overlay_note.unwrap_or(60);
        let pitch_class = note.rem_euclid(12) as u8;
        let (name, sharp) = pitch_class_name(pitch_class);

        let light = if sharp {
            if (1..=3).contains(&col) {
                if note_letter_pixel(name, row, col - 1) {
                    Light::White
                } else {
                    Light::Off
                }
            } else if (5..=6).contains(&col) {
                if sharp_pixel(row, col - 5) {
                    Light::BlueDark
                } else {
                    Light::Off
                }
            } else {
                Light::Off
            }
        } else if (2..=4).contains(&col) {
            if note_letter_pixel(name, row, col - 2) {
                Light::White
            } else {
                Light::Off
            }
        } else {
            Light::Off
        };

        Some(light)
    }

    fn lfo_speed_overlay_active(&self) -> bool {
        let params = self.params.lock().unwrap();
        if let Some(until) = params.lfo_speed_overlay_until {
            Instant::now() <= until && params.lfo_speed_overlay_value.is_some()
        } else {
            false
        }
    }

    fn lfo_wave_overlay_active(&self) -> bool {
        let params = self.params.lock().unwrap();
        if let Some(until) = params.lfo_wave_overlay_until {
            Instant::now() <= until && params.lfo_wave_overlay_mode.is_some()
        } else {
            false
        }
    }

    fn tempo_overlay_active(&self) -> bool {
        if let Some(until) = self.tempo_overlay_until {
            Instant::now() <= until && self.tempo_overlay_value.is_some()
        } else {
            false
        }
    }

    fn control_overlay_active(&self) -> bool {
        if let Some(until) = self.control_overlay_until {
            Instant::now() <= until && self.control_overlay_label.is_some()
        } else {
            false
        }
    }

    fn control_overlay_display_light(&self, row: usize, col: usize) -> Option<Light> {
        if !self.control_overlay_active() {
            return None;
        }

        let label = self.control_overlay_label?;
        let value = self.control_overlay_value.as_deref();
        Some(control_overlay_pixel(
            label,
            value,
            self.control_overlay_mode,
            row,
            col,
        ))
    }

    fn tempo_overlay_display_light(&self, row: usize, col: usize) -> Option<Light> {
        if self.root_overlay_active() {
            return self.root_overlay_display_light(row, col);
        }

        if self.lfo_speed_overlay_active() {
            let params = self.params.lock().unwrap();
            return Some(control_overlay_pixel(
                "LFO_SPEED",
                params.lfo_speed_overlay_value.as_deref(),
                None,
                row,
                col,
            ));
        }

        if self.lfo_wave_overlay_active() {
            let params = self.params.lock().unwrap();
            return Some(control_overlay_pixel(
                "LFO_WAVE",
                None,
                params.lfo_wave_overlay_mode,
                row,
                col,
            ));
        }

        if self.control_overlay_active() {
            return self.control_overlay_display_light(row, col);
        }

        if !self.tempo_overlay_active() {
            return None;
        }

        let value = self.tempo_overlay_value?;
        let hundreds = (value / 100) as u8;
        let tens = ((value / 10) % 10) as u8;
        let ones = (value % 10) as u8;

        let light = if col == 0 {
            if value >= 100 && narrow_digit_pixel(hundreds, row) {
                Light::Green
            } else {
                Light::Off
            }
        } else if (2..=4).contains(&col) {
            if wide_digit_pixel(tens, row, col - 2) {
                Light::White
            } else {
                Light::Off
            }
        } else if (5..=7).contains(&col) {
            if wide_digit_pixel(ones, row, col - 5) {
                Light::Green
            } else {
                Light::Off
            }
        } else {
            Light::Off
        };

        Some(light)
    }

    fn display_overlay_light(&self, row: usize, col: usize) -> Option<Light> {
        self.tempo_overlay_display_light(row, col)
    }

    fn tempo_overlay_light(&self, id: u32) -> Option<Light> {
        let coords = Coords::from(id);
        if coords.row < 10 || coords.row > 13 {
            return None;
        }

        let row = (coords.row - 9) as usize;
        let col = coords.col as usize;
        self.display_overlay_light(row, col)
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

        for id in 0..PLAYABLE_GRID_SIZE {
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
        let new_state = if self.select_held {
            Light::Green
        } else if self.selection.len() > 0 {
            Light::GreenLow
        } else {
            Light::OrangeLow
        };

        if self.select_out != new_state {
            self.send_note_color(1, SELECT_BUTTON, new_state);
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
        self.refresh_loop_button();
    }

    fn tap_tempo(&mut self) {
        // TODO: make work
        // clock_sender.send(ToClock::TapTempo).unwrap();
    }

    fn start_loop(&mut self) {
        self.commit_selection_override();
        self.loop_held = true;
        self.loop_from = self.last_pos.round();
        self.loop_button_out = Light::Off;
        self.refresh_loop_button();
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

        for id in 0..PLAYABLE_GRID_SIZE {
            // include ids that are recording, or if select is held, all active IDs!
            let selected = self.select_held || self.selection.contains(&id);
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

        self.last_loop_snapshot = Some(LoopSnapshot {
            loop_from: self.loop_from,
            loop_length: self.loop_length,
            last_changed_triggers: self.last_changed_triggers.clone(),
            recording_ids,
        });

        if new_loop.transforms.len() > 0 {
            new_loop.length = self.loop_length;
            self.loop_state.set(new_loop);
            self.clear_recording();
        }

        self.refresh_loop_length();
        self.clear_selection();
    }

    fn reloop(&mut self, length: MidiTime) {
        if let Some(last_loop_snapshot) = &self.last_loop_snapshot {
            let offset = last_loop_snapshot.loop_length - length;
            let pos = self.loop_from + offset;

            let mut recording_ids = HashSet::new();

            for id in &last_loop_snapshot.recording_ids {
                recording_ids.insert(*id);
            }

            // add additional IDs from before the old loop start
            for (id, last_change) in &last_loop_snapshot.last_changed_triggers {
                if last_change > &pos {
                    recording_ids.insert(*id);
                }
            }

            self.loop_state.undo();

            let mut current_loop = self.loop_state.get().clone();

            for id in recording_ids {
                let current_event = self.recorder.get_event_at(id, pos);
                let has_events = self.recorder.has_events(id, pos, pos + length);
                if has_events {
                    current_loop
                        .transforms
                        .insert(id, LoopTransform::Range { pos, length });
                } else if let Some(event) = current_event {
                    current_loop
                        .transforms
                        .insert(id, LoopTransform::Value(event.value));
                } else {
                    current_loop.transforms.insert(id, LoopTransform::None);
                }
            }

            self.loop_state.set(current_loop);
        }
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
            self.send_note_color(1, FLATTEN_BUTTON, color);
        }
    }

    fn flatten(&mut self) {
        let mut new_loop = self.loop_state.get().clone();

        for id in 0..PLAYABLE_GRID_SIZE {
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
        self.clear_loop_snapshot();
    }

    fn clear_loops(&mut self, target: TransformTarget, clear_permanent: bool) {
        let mut new_loop = self.loop_state.get().clone();

        let ids: Vec<u32> = match target {
            TransformTarget::All | TransformTarget::Main => (0..PLAYABLE_GRID_SIZE).collect(),
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
        self.clear_loop_snapshot();
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

        for id in 0..PLAYABLE_GRID_SIZE {
            self.refresh_override(id);
        }

        self.refresh_should_flatten();

        let mut params = self.params.lock().unwrap();
        params.frozen = pressed;
        params.cueing = false;
    }

    fn set_rate(&mut self, value: MidiTime) {
        self.rate = value;
        self.repeat_button_out = Light::Off;
        self.loop_button_out = Light::Off;
        self.show_control_overlay("RATE", Some(rate_overlay_value(value).to_string()));
        self.refresh_loop_length();
        self.refresh_loop_button();
        self.refresh_repeat_button();
        self.refresh_selection_override();
        self.refresh_override_repeat();
    }

    fn set_swing(&mut self, value: u8) {
        let mut params = self.params.lock().unwrap();
        let linear_swing = value as f64 / 127.0;
        params.swing = linear_swing.powf(2.0);
        drop(params);
        self.show_control_overlay("SWING", Some(swing_overlay_value(value)));
    }

    fn set_tempo(&mut self, value: u8) {
        let bpm = tempo_from_cc(value);
        *self.internal_bpm.lock().unwrap() = bpm;
        self.tempo_overlay_value = Some(bpm.round().min(199.0) as u16);
        self.tempo_overlay_until = Some(Instant::now() + Duration::from_millis(1200));
    }

    fn set_trigger_mode(&mut self, value: TriggerMode) {
        self.trigger_mode = value;
        self.show_control_overlay("TRG", Some(trigger_mode_overlay_value(value).to_string()));
        self.refresh_loop_button();
        self.refresh_prepare_button();
        self.refresh_override_repeat();
        self.refresh_all_inputs();
    }

    fn set_trigger_override_held(&mut self, pressed: bool) {
        if self.trigger_override_held == pressed {
            return;
        }

        self.trigger_override_held = pressed;
        self.params.lock().unwrap().prepare_held = pressed;
        self.show_control_overlay(
            "TRG",
            Some(trigger_mode_overlay_value(self.effective_trigger_mode()).to_string()),
        );
        self.refresh_loop_button();
        self.refresh_prepare_button();
        self.refresh_override_repeat();
        self.refresh_all_inputs();
    }

    fn effective_trigger_mode(&self) -> TriggerMode {
        if self.trigger_override_held {
            self.trigger_mode.override_mode()
        } else {
            self.trigger_mode
        }
    }

    fn effective_trigger_mode_for_id(&self, id: u32) -> TriggerMode {
        if self.preserve_immediate_until_release.contains(&id) {
            TriggerMode::Immediate
        } else {
            self.effective_trigger_mode()
        }
    }

    fn show_control_overlay(&mut self, label: &'static str, value: Option<String>) {
        self.show_control_overlay_with_mode(label, value, None);
    }

    fn show_control_overlay_with_mode(
        &mut self,
        label: &'static str,
        value: Option<String>,
        mode: Option<&'static str>,
    ) {
        self.control_overlay_label = Some(label);
        self.control_overlay_value = value;
        self.control_overlay_mode = mode;
        self.control_overlay_until = Some(Instant::now() + Duration::from_millis(900));
    }

    fn show_peek_overlay_for_bank(&mut self, bank_id: usize) {
        match bank_id {
            0 => self.show_control_overlay("RATE", Some(rate_overlay_value(self.rate).to_string())),
            1 => self.show_control_overlay(
                "TRG",
                Some(trigger_mode_overlay_value(self.trigger_mode).to_string()),
            ),
            2 => {
                let swing_value = {
                    let params = self.params.lock().unwrap();
                    float_to_swing_cc(params.swing)
                };
                self.show_control_overlay("SWING", Some(swing_overlay_value(swing_value)));
            }
            3 => {
                let bpm = *self.internal_bpm.lock().unwrap();
                self.tempo_overlay_value = Some(bpm.round().min(199.0) as u16);
                self.tempo_overlay_until = Some(Instant::now() + Duration::from_millis(900));
            }
            _ => {}
        }
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
        for id in 0..PLAYABLE_GRID_SIZE {
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
                let pos = self.pos_for_id(id);
                self.out_transforms.insert(id, transform);
                self.last_changed_triggers.insert(id, pos);

                // send new value
                if let Some(value) = self.get_value(id, pos, None) {
                    self.event(LoopEvent { id, value, pos });
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
                    params
                        .activity_flash_until
                        .insert(*channel, Instant::now() + Duration::from_millis(100));
                }

                if let Some(trigger_channels) = self.chunk_trigger_channels.get(&map.chunk_index) {
                    if let Some(channel) = trigger_channels.get(map.id as usize) {
                        let mut params = self.params.lock().unwrap();
                        params.channel_triggered.insert(*channel);
                        params
                            .activity_flash_until
                            .insert(*channel, Instant::now() + Duration::from_millis(100));
                    }
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
                    self.clear_loop_snapshot();
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

        for (id, transform) in &self.out_transforms {
            let position = self.pos_for_id(*id);
            let length = self.length_for_id(*id);

            if length > MidiTime::zero() {
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

    fn pos_for_id(&self, id: u32) -> MidiTime {
        if self.straight_timing_ids.contains(&id) {
            self.last_raw_pos
        } else {
            self.last_pos
        }
    }

    fn length_for_id(&self, id: u32) -> MidiTime {
        if self.straight_timing_ids.contains(&id) {
            self.last_raw_length
        } else {
            self.last_length
        }
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

    for id in 0..PLAYABLE_GRID_SIZE {
        let midi = if id < 80 {
            GRID_NOTE_START + id as u8
        } else {
            GRID_2_NOTE_START + (id as u8 - 80)
        };
        midi_to_id.insert(midi, id);
        id_to_midi.insert(id, midi);
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
enum GridLight {
    Constant(Light),
    Pulsing(Light),
}

fn send_digital_feedback_frame(
    output: &mut midi_connection::SharedMidiOutputConnection,
    records: &[(u16, DigitalFeedback)],
) {
    if records.is_empty() {
        return;
    }

    let first_index = records.first().map(|(index, _)| *index).unwrap_or(0);
    let last_index = records.last().map(|(index, _)| *index).unwrap_or(0);

    let mut message =
        Vec::with_capacity(BULK_DIGITAL_FEEDBACK_FRAME_PREFIX.len() + records.len() * 4 + 1);
    message.extend_from_slice(&BULK_DIGITAL_FEEDBACK_FRAME_PREFIX);
    message[9] = records.len() as u8;

    for (index, feedback) in records {
        message.push((index & 0x7F) as u8);
        message.push(((index >> 7) & 0x7F) as u8);
        message.push(feedback.color);
        message.push(feedback.intensity);
    }

    message.push(0xF7);
    if let Err(err) = output.send(&message) {
        println!(
            "[WARN] Failed to queue grid digital bulk frame: count={}, first_index={}, last_index={}, err={:?}",
            records.len(), first_index, last_index, err
        );
    }
}

fn digital_index_for_note(channel: u8, note: u8) -> Option<u16> {
    match (channel, note) {
        (1, 8) => Some(0),
        (1, 9) => Some(1),
        (1, 10) => Some(2),
        (1, 11) => Some(3),
        (1, 16) => Some(4),
        (1, 17) => Some(5),
        (1, 18) => Some(6),
        (1, 19) => Some(7),
        (1, 12) => Some(8),
        (1, 13) => Some(9),
        (1, 14) => Some(10),
        (1, 15) => Some(11),
        (1, 20) => Some(12),
        (1, 21) => Some(13),
        (1, 22) => Some(14),
        (1, 23) => Some(15),
        (2, 61) => Some(16),
        (2, 62) => Some(17),
        (2, 63) => Some(18),
        (2, 64) => Some(19),
        (2, 65) => Some(20),
        (2, 66) => Some(21),
        (2, 67) => Some(22),
        (2, 68) => Some(23),
        (1, 88) => Some(24),
        (1, 89) => Some(25),
        (1, 90) => Some(26),
        (1, 91) => Some(27),
        (1, 92) => Some(28),
        (1, 93) => Some(29),
        (1, 94) => Some(30),
        (1, 95) => Some(31),
        (1, 24) => Some(32),
        (1, 25) => Some(33),
        (1, 26) => Some(34),
        (1, 27) => Some(35),
        (1, 32) => Some(36),
        (1, 33) => Some(37),
        (1, 34) => Some(38),
        (1, 35) => Some(39),
        (1, 28) => Some(40),
        (1, 29) => Some(41),
        (1, 30) => Some(42),
        (1, 31) => Some(43),
        (1, 36) => Some(44),
        (1, 37) => Some(45),
        (1, 38) => Some(46),
        (1, 39) => Some(47),
        (1, 96) => Some(48),
        (1, 97) => Some(49),
        (1, 98) => Some(50),
        (1, 99) => Some(51),
        (1, 100) => Some(52),
        (1, 101) => Some(53),
        (1, 102) => Some(54),
        (1, 103) => Some(55),
        (1, 104) => Some(56),
        (1, 105) => Some(57),
        (1, 106) => Some(58),
        (1, 107) => Some(59),
        (1, 108) => Some(60),
        (1, 109) => Some(61),
        (1, 110) => Some(62),
        (1, 111) => Some(63),
        (1, 40) => Some(64),
        (1, 41) => Some(65),
        (1, 42) => Some(66),
        (1, 43) => Some(67),
        (1, 48) => Some(68),
        (1, 49) => Some(69),
        (1, 50) => Some(70),
        (1, 51) => Some(71),
        (1, 44) => Some(72),
        (1, 45) => Some(73),
        (1, 46) => Some(74),
        (1, 47) => Some(75),
        (1, 52) => Some(76),
        (1, 53) => Some(77),
        (1, 54) => Some(78),
        (1, 55) => Some(79),
        (1, 112) => Some(80),
        (1, 113) => Some(81),
        (1, 114) => Some(82),
        (1, 115) => Some(83),
        (1, 116) => Some(84),
        (1, 117) => Some(85),
        (1, 118) => Some(86),
        (1, 119) => Some(87),
        (1, 120) => Some(88),
        (1, 121) => Some(89),
        (1, 122) => Some(90),
        (1, 123) => Some(91),
        (1, 124) => Some(92),
        (1, 125) => Some(93),
        (1, 126) => Some(94),
        (1, 127) => Some(95),
        (1, 56) => Some(96),
        (1, 57) => Some(97),
        (1, 58) => Some(98),
        (1, 59) => Some(99),
        (1, 64) => Some(100),
        (1, 65) => Some(101),
        (1, 66) => Some(102),
        (1, 67) => Some(103),
        (1, 60) => Some(104),
        (1, 61) => Some(105),
        (1, 62) => Some(106),
        (1, 63) => Some(107),
        (1, 68) => Some(108),
        (1, 69) => Some(109),
        (1, 70) => Some(110),
        (1, 71) => Some(111),
        (1, 72) => Some(112),
        (1, 73) => Some(113),
        (1, 74) => Some(114),
        (1, 75) => Some(115),
        (1, 80) => Some(116),
        (1, 81) => Some(117),
        (1, 82) => Some(118),
        (1, 83) => Some(119),
        (1, 76) => Some(120),
        (1, 77) => Some(121),
        (1, 78) => Some(122),
        (1, 79) => Some(123),
        (1, 84) => Some(124),
        (1, 85) => Some(125),
        (1, 86) => Some(126),
        (1, 87) => Some(127),
        (1, 0) => Some(128),
        (1, 1) => Some(129),
        (1, 2) => Some(130),
        (1, 3) => Some(131),
        (1, 4) => Some(132),
        (1, 5) => Some(133),
        (1, 6) => Some(134),
        (1, 7) => Some(135),
        (2, 1) => Some(136),
        (2, 2) => Some(137),
        (2, 3) => Some(138),
        (2, 4) => Some(139),
        _ => None,
    }
}

fn light_to_hue(light: Light) -> u8 {
    match light {
        Light::Off | Light::None => 0,
        Light::White | Light::WhiteMed => 127,
        Light::Yellow | Light::YellowMed => 19,
        Light::Lime | Light::LimeLow => 31,
        Light::Purple => 95,
        Light::Green | Light::GreenMed | Light::GreenLow | Light::GreenDark => 40,
        Light::Orange | Light::OrangeMed | Light::OrangeLow => 13,
        Light::Red | Light::RedMed | Light::RedLow => 1,
        Light::BlueDark => 85,
        Light::Value(value) | Light::ValueLow(value) => value,
    }
}

fn light_to_intensity(light: Light) -> u8 {
    match light {
        Light::Off | Light::None => 0,
        Light::GreenDark => 24,
        Light::GreenLow
        | Light::LimeLow
        | Light::RedLow
        | Light::OrangeLow
        | Light::ValueLow(..) => 48,
        Light::GreenMed | Light::YellowMed | Light::RedMed | Light::OrangeMed | Light::WhiteMed => {
            96
        }
        _ => 127,
    }
}

fn pitch_class_name(pitch_class: u8) -> (char, bool) {
    match pitch_class % 12 {
        0 => ('C', false),
        1 => ('C', true),
        2 => ('D', false),
        3 => ('D', true),
        4 => ('E', false),
        5 => ('F', false),
        6 => ('F', true),
        7 => ('G', false),
        8 => ('G', true),
        9 => ('A', false),
        10 => ('A', true),
        _ => ('B', false),
    }
}

fn control_overlay_pixel(
    label: &str,
    value: Option<&str>,
    mode: Option<&str>,
    row: usize,
    col: usize,
) -> Light {
    match label {
        "TRG" => trigger_mode_overlay_pixel(value, row, col),
        "RATE" => {
            if let Some(value) = value {
                let light = if value.ends_with('T') {
                    Light::Purple
                } else {
                    Light::Yellow
                };
                compact_text_pixel(value, row, col, light)
            } else {
                Light::Off
            }
        }
        "LFO_WAVE" => lfo_wave_overlay_pixel(mode, row, col),
        _ => {
            if let Some(value) = value {
                compact_text_pixel(value, row, col, Light::White)
            } else {
                Light::Off
            }
        }
    }
}

fn compact_text_pixel(text: &str, row: usize, col: usize, light: Light) -> Light {
    let chars: Vec<char> = text.chars().collect();
    let start_col = compact_text_start(chars.len());

    for (index, ch) in chars.iter().enumerate() {
        let glyph_start = start_col + (index * 4);
        if col >= glyph_start && col < glyph_start + 3 {
            if compact_glyph_pixel(*ch, row, col - glyph_start) {
                return light;
            } else {
                return Light::Off;
            }
        }
    }

    Light::Off
}

fn compact_text_start(len: usize) -> usize {
    let width = if len == 0 { 0 } else { (len * 3) + (len - 1) };
    if width >= 8 {
        0
    } else {
        (8 - width) / 2
    }
}

fn compact_glyph_pixel(glyph: char, row: usize, col: usize) -> bool {
    match glyph {
        '0'..='9' => wide_digit_pixel(glyph.to_digit(10).unwrap() as u8, row, col),
        'A' | 'B' | 'Q' | 'R' | 'T' => note_letter_pixel(glyph, row, col),
        _ => false,
    }
}

fn note_letter_pixel(letter: char, row: usize, col: usize) -> bool {
    const C: [[bool; 3]; 5] = [
        [true, true, true],
        [true, false, false],
        [true, false, false],
        [true, false, false],
        [true, true, true],
    ];
    const D: [[bool; 3]; 5] = [
        [true, true, false],
        [true, false, true],
        [true, false, true],
        [true, false, true],
        [true, true, false],
    ];
    const L: [[bool; 3]; 5] = [
        [true, false, false],
        [true, false, false],
        [true, false, false],
        [true, false, false],
        [true, true, true],
    ];
    const P: [[bool; 3]; 5] = [
        [true, true, false],
        [true, false, true],
        [true, true, false],
        [true, false, false],
        [true, false, false],
    ];
    const Q: [[bool; 3]; 5] = [
        [true, true, true],
        [true, false, true],
        [true, false, true],
        [true, true, true],
        [false, false, true],
    ];
    const E: [[bool; 3]; 5] = [
        [true, true, true],
        [true, false, false],
        [true, true, false],
        [true, false, false],
        [true, true, true],
    ];
    const F: [[bool; 3]; 5] = [
        [true, true, true],
        [true, false, false],
        [true, true, false],
        [true, false, false],
        [true, false, false],
    ];
    const G: [[bool; 3]; 5] = [
        [true, true, true],
        [true, false, false],
        [true, false, true],
        [true, false, true],
        [true, true, true],
    ];
    const I: [[bool; 3]; 5] = [
        [true, true, true],
        [false, true, false],
        [false, true, false],
        [false, true, false],
        [true, true, true],
    ];
    const N: [[bool; 3]; 5] = [
        [true, false, true],
        [true, true, true],
        [true, true, true],
        [true, false, true],
        [true, false, true],
    ];
    const R: [[bool; 3]; 5] = [
        [true, true, false],
        [true, false, true],
        [true, true, false],
        [true, false, true],
        [true, false, true],
    ];
    const S: [[bool; 3]; 5] = [
        [true, true, true],
        [true, false, false],
        [true, true, true],
        [false, false, true],
        [true, true, true],
    ];
    const T: [[bool; 3]; 5] = [
        [true, true, true],
        [false, true, false],
        [false, true, false],
        [false, true, false],
        [false, true, false],
    ];
    const U: [[bool; 3]; 5] = [
        [true, false, true],
        [true, false, true],
        [true, false, true],
        [true, false, true],
        [true, true, true],
    ];
    const W: [[bool; 3]; 5] = [
        [true, false, true],
        [true, false, true],
        [true, true, true],
        [true, true, true],
        [true, false, true],
    ];
    const A: [[bool; 3]; 5] = [
        [true, true, true],
        [true, false, true],
        [true, true, true],
        [true, false, true],
        [true, false, true],
    ];
    const B: [[bool; 3]; 5] = [
        [true, true, false],
        [true, false, true],
        [true, true, false],
        [true, false, true],
        [true, true, false],
    ];
    const Z: [[bool; 3]; 5] = [
        [true, true, true],
        [false, false, true],
        [false, true, false],
        [true, false, false],
        [true, true, true],
    ];

    let pixels = match letter {
        'C' => &C,
        'D' => &D,
        'E' => &E,
        'F' => &F,
        'G' => &G,
        'I' => &I,
        'L' => &L,
        'N' => &N,
        'P' => &P,
        'Q' => &Q,
        'R' => &R,
        'S' => &S,
        'T' => &T,
        'U' => &U,
        'W' => &W,
        'A' => &A,
        'B' => &B,
        '1' => &I,
        '2' => &Z,
        '3' => &E,
        '4' => &A,
        '5' => &S,
        '6' => &G,
        '8' => &B,
        _ => return false,
    };

    pixels
        .get(row)
        .and_then(|cols| cols.get(col))
        .copied()
        .unwrap_or(false)
}

fn sharp_pixel(row: usize, col: usize) -> bool {
    const SHARP: [[bool; 2]; 5] = [
        [true, false],
        [true, true],
        [true, false],
        [true, true],
        [true, false],
    ];

    SHARP
        .get(row)
        .and_then(|cols| cols.get(col))
        .copied()
        .unwrap_or(false)
}

fn narrow_digit_pixel(digit: u8, row: usize) -> bool {
    match digit {
        1 => row < 5,
        _ => false,
    }
}

fn wide_digit_pixel(digit: u8, row: usize, col: usize) -> bool {
    const DIGITS: [[[bool; 3]; 5]; 10] = [
        [
            [true, true, true],
            [true, false, true],
            [true, false, true],
            [true, false, true],
            [true, true, true],
        ],
        [
            [false, true, false],
            [true, true, false],
            [false, true, false],
            [false, true, false],
            [true, true, true],
        ],
        [
            [true, true, true],
            [false, false, true],
            [true, true, true],
            [true, false, false],
            [true, true, true],
        ],
        [
            [true, true, true],
            [false, false, true],
            [true, true, true],
            [false, false, true],
            [true, true, true],
        ],
        [
            [true, false, true],
            [true, false, true],
            [true, true, true],
            [false, false, true],
            [false, false, true],
        ],
        [
            [true, true, true],
            [true, false, false],
            [true, true, true],
            [false, false, true],
            [true, true, true],
        ],
        [
            [true, true, true],
            [true, false, false],
            [true, true, true],
            [true, false, true],
            [true, true, true],
        ],
        [
            [true, true, true],
            [false, false, true],
            [false, false, true],
            [false, false, true],
            [false, false, true],
        ],
        [
            [true, true, true],
            [true, false, true],
            [true, true, true],
            [true, false, true],
            [true, true, true],
        ],
        [
            [true, true, true],
            [true, false, true],
            [true, true, true],
            [false, false, true],
            [true, true, true],
        ],
    ];

    DIGITS
        .get(digit as usize)
        .and_then(|rows| rows.get(row))
        .and_then(|cols| cols.get(col))
        .copied()
        .unwrap_or(false)
}

fn trigger_mode_overlay_value(mode: TriggerMode) -> &'static str {
    match mode {
        TriggerMode::Immediate => "T",
        TriggerMode::Quantized => "Q",
        TriggerMode::Repeat => "R",
        TriggerMode::Cycle => "A",
    }
}

fn trigger_mode_light(mode: TriggerMode) -> Light {
    match mode {
        TriggerMode::Immediate => Light::Green,
        TriggerMode::Quantized => Light::Yellow,
        TriggerMode::Repeat => Light::Purple,
        TriggerMode::Cycle => Light::BlueDark,
    }
}

fn trigger_mode_button_light(mode: TriggerMode, override_active: bool) -> Light {
    let light = trigger_mode_light(mode);
    if override_active {
        light
    } else {
        Light::ValueLow(light_to_hue(light))
    }
}

fn lfo_wave_overlay_pixel(mode: Option<&str>, row: usize, col: usize) -> Light {
    let wave = match mode {
        Some("TRI") => Some((
            [
                [false, false, true, false, false, false, false, false],
                [false, true, false, true, false, false, false, false],
                [true, false, false, false, true, false, false, false],
                [false, false, false, false, false, true, false, true],
                [false, false, false, false, false, false, true, false],
            ],
            Light::White,
        )),
        Some("UP") => Some((
            [
                [false, false, false, false, true, false, false, false],
                [false, false, false, true, true, false, false, true],
                [false, false, true, false, true, false, true, false],
                [false, true, false, false, true, true, false, false],
                [true, false, false, false, true, false, false, false],
            ],
            Light::Yellow,
        )),
        Some("HUP") => Some((
            [
                [false, false, false, false, true, true, true, true],
                [false, false, false, true, false, false, false, true],
                [false, false, true, false, false, false, false, true],
                [false, true, false, false, false, false, false, true],
                [true, false, false, false, false, false, false, true],
            ],
            Light::Purple,
        )),
        Some("HDN") => Some((
            [
                [true, true, true, true, false, false, false, false],
                [true, false, false, false, true, false, false, false],
                [true, false, false, false, false, true, false, false],
                [true, false, false, false, false, false, true, false],
                [true, false, false, false, false, false, false, true],
            ],
            Light::Purple,
        )),
        Some("DWN") => Some((
            [
                [true, false, false, false, true, false, false, false],
                [false, true, false, false, true, true, false, false],
                [false, false, true, false, true, false, true, false],
                [false, false, false, true, true, false, false, true],
                [false, false, false, false, true, false, false, false],
            ],
            Light::BlueDark,
        )),
        _ => None,
    };

    if let Some((pixels, light)) = wave {
        if row >= 5 || col >= 8 {
            return Light::Off;
        }
        if pixels[row][col] {
            light
        } else {
            Light::Off
        }
    } else {
        Light::Off
    }
}

fn trigger_mode_overlay_pixel(value: Option<&str>, row: usize, col: usize) -> Light {
    let mode = match value {
        Some("T") => Some((
            [
                [false, false, true, false, false],
                [false, false, true, true, false],
                [true, true, true, true, true],
                [false, false, true, true, false],
                [false, false, true, false, false],
            ],
            trigger_mode_light(TriggerMode::Immediate),
        )),
        Some("Q") => Some((
            [
                [false, true, true, true, false],
                [true, false, false, false, true],
                [true, false, true, false, true],
                [true, false, false, false, true],
                [false, true, true, true, false],
            ],
            trigger_mode_light(TriggerMode::Quantized),
        )),
        Some("R") => Some((
            [
                [true, false, true, false, true],
                [true, false, true, false, true],
                [true, false, true, false, true],
                [true, false, true, false, true],
                [true, false, true, false, true],
            ],
            trigger_mode_light(TriggerMode::Repeat),
        )),
        Some("A") => Some((
            [
                [true, false, false, false, false],
                [true, true, false, false, false],
                [false, true, true, false, false],
                [false, false, true, true, false],
                [false, false, false, true, true],
            ],
            trigger_mode_light(TriggerMode::Cycle),
        )),
        _ => None,
    };

    if let Some((pixels, light)) = mode {
        let start_col = 1usize;
        if col < start_col || col >= start_col + 5 {
            return Light::Off;
        }
        if row >= 5 {
            return Light::Off;
        }
        if pixels[row][col - start_col] {
            light
        } else {
            Light::Off
        }
    } else {
        Light::Off
    }
}

fn swing_overlay_value(value: u8) -> String {
    let amount = ((value as f64 / 127.0) * 50.0).round() as u8;
    amount.to_string()
}

fn rate_overlay_value(rate: MidiTime) -> &'static str {
    if rate == MidiTime::from_measure(2, 1) {
        "2B"
    } else if rate == MidiTime::from_measure(1, 1) {
        "1B"
    } else if rate == MidiTime::from_measure(1, 2) {
        "2"
    } else if rate == MidiTime::from_measure(1, 4) {
        "4"
    } else if rate == MidiTime::from_measure(1, 8) {
        "8"
    } else if rate == MidiTime::from_measure(1, 6) {
        "4T"
    } else if rate == MidiTime::from_measure(1, 3) {
        "2T"
    } else if rate == MidiTime::from_measure(2, 3) {
        "1T"
    } else {
        "RATE"
    }
}

fn float_to_swing_cc(value: f64) -> u8 {
    (value.max(0.0).min(1.0).sqrt() * 127.0).round() as u8
}

fn tempo_from_cc(value: u8) -> f64 {
    let center_min = 80.0;
    let center_max = 180.0;
    if value <= 15 {
        40.0 + (value as f64 / 15.0) * (center_min - 40.0)
    } else if value >= 112 {
        center_max + ((value as f64 - 112.0) / 15.0) * (200.0 - center_max)
    } else {
        center_min + ((value as f64 - 16.0) / 95.0) * (center_max - center_min)
    }
}

fn is_triplet_rate(rate: MidiTime) -> bool {
    rate == MidiTime::from_measure(2, 3)
        || rate == MidiTime::from_measure(1, 3)
        || rate == MidiTime::from_measure(1, 6)
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

fn cc_bucket(value: u8, count: usize) -> usize {
    ((value as usize * count) / 128).min(count.saturating_sub(1))
}
