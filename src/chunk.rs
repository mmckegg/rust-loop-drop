pub use std::time::{SystemTime, Duration};
pub use ::output_value::OutputValue;
pub use ::midi_time::MidiTime;
use std::collections::HashSet;

pub trait Triggerable {
    // TODO: or should this be MidiTime??
    fn trigger (&mut self, id: u32, value: OutputValue);
    fn on_tick (&mut self, _time: MidiTime) {}
    fn get_active (&self) -> Option<HashSet<u32>> { None }
    fn latch_mode (&self) -> LatchMode { LatchMode::None }
    fn schedule_mode (&self) -> ScheduleMode { ScheduleMode::MostRecent }  
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
pub struct Coords {
    pub row: u32,
    pub col: u32
}

impl Coords {
    pub fn new (row: u32, col: u32) -> Coords {
        Coords { row, col }
    }

    pub fn from (id: u32) -> Coords {
        Coords {
            row: id / 8, 
            col: id % 8
        }
    }

    pub fn id_from (row: u32, col: u32) -> u32 {
        (row * 8) + col
    }

    // pub fn id (&self) -> u32 {
    //     Coords::id_from(self.row, self.col)
    // }
}

pub struct Shape {
    pub rows: u32,
    pub cols: u32
}

impl Shape {
    pub fn new (rows: u32, cols: u32) -> Shape {
        Shape { rows, cols }
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
pub struct MidiMap {
    pub chunk_index: usize,
    pub id: u32
}

pub struct ChunkMap {
    pub coords: Coords,
    pub shape: Shape,
    pub chunk: Box<Triggerable + Send>,
    pub channel: Option<u32>,
    pub color: u8,
    pub repeat_mode: RepeatMode
}

impl ChunkMap {
    pub fn new (chunk: Box<Triggerable + Send>, coords: Coords, shape: Shape, color: u8, channel: Option<u32>, repeat_mode: RepeatMode) -> Box<Self> {
        Box::new(ChunkMap {
            chunk, coords, shape, color, channel, repeat_mode
        })
    }
}

pub enum TriggerModeChange {
    Selected(u32, bool),
    SelectingScale(bool),
    Active(u32, bool)
}

pub enum RepeatMode {
    Global,
    None
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
pub enum LatchMode {
    None,
    LatchSingle,
    NoSuppress
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
pub enum ScheduleMode {
    MostRecent,
    Monophonic,
    Percussion
}