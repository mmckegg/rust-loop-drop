use chunk::{LatchMode, MidiTime, OutputValue, ScheduleMode, Triggerable};
use std::collections::HashSet;

pub use scale::{Offset, Scale};

pub struct MultiChunk {
    chunks: Vec<Box<dyn Triggerable + Send>>,
}

impl MultiChunk {
    pub fn new(chunks: Vec<Box<dyn Triggerable + Send>>) -> Self {
        MultiChunk { chunks }
    }
}

impl Triggerable for MultiChunk {
    fn trigger(&mut self, id: u32, value: OutputValue) {
        for chunk in self.chunks.iter_mut() {
            chunk.trigger(id, value);
        }
    }

    fn on_tick(&mut self, time: MidiTime) {
        for chunk in self.chunks.iter_mut() {
            chunk.on_tick(time);
        }
    }

    // pass thru to first chunk
    fn check_lit(&self, id: u32) -> bool {
        self.chunks[0].check_lit(id)
    }

    fn get_notes(&self) -> Option<HashSet<u8>> {
        self.chunks[0].get_notes()
    }

    // pass thru to first chunk
    fn check_triggering(&self, id: u32) -> Option<bool> {
        self.chunks[0].check_triggering(id)
    }
    fn latch_mode(&self) -> LatchMode {
        self.chunks[0].latch_mode()
    }
    fn schedule_mode(&self) -> ScheduleMode {
        self.chunks[0].schedule_mode()
    }
}
