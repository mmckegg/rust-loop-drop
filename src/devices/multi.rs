use ::chunk::{Triggerable, OutputValue, MidiTime, LatchMode, ScheduleMode};
use std::collections::HashSet;

pub use ::scale::{Scale, Offset};

pub struct MultiChunk {
    chunks: Vec<Box<dyn Triggerable + Send>>
}

impl MultiChunk {
    pub fn new (chunks: Vec<Box<dyn Triggerable + Send>>) -> Self {
        MultiChunk {
            chunks
        }
    }
}

impl Triggerable for MultiChunk {
    fn trigger (&mut self, id: u32, value: OutputValue) {
        for chunk in self.chunks.iter_mut() {
            chunk.trigger(id, value);
        }
    }

    fn on_tick (&mut self, time: MidiTime) {
        for chunk in self.chunks.iter_mut() {
            chunk.on_tick(time);
        }
    }

    // pass thru to first chunk
    fn get_active (&self) -> Option<HashSet<u32>> { 
        self.chunks[0].get_active()
     }
    fn latch_mode (&self) -> LatchMode { 
        self.chunks[0].latch_mode()
    }
    fn schedule_mode (&self) -> ScheduleMode { 
        self.chunks[0].schedule_mode()
    }  
}