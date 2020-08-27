extern crate circular_queue;
use self::circular_queue::CircularQueue;

use std::time::{Duration, Instant};
use std::thread;
use midi_connection;
use std::sync::{Arc, Mutex};


pub use ::midi_time::{MidiTime, SUB_TICKS};

pub struct Scheduler {
    next_pos: MidiTime,
    remote_state: Arc<Mutex<RemoteSchedulerState>>,
    _clock_source: Option<midi_connection::ThreadReference>
}

struct RemoteSchedulerState {
    ticked: bool,
    restarted: bool,
}

impl Scheduler {
    pub fn start (clock_port_name: &str) -> Self {
        let remote_state = Arc::new(Mutex::new(RemoteSchedulerState {
            ticked: false,
            restarted: false
        }));

        // track external clock and tick durations (to calculate bpm)
        let state_m = remote_state.clone();
        let _clock_source = Some(midi_connection::get_input(clock_port_name, move |stamp, message| {
            
            if message[0] == 248 {
                let mut state: std::sync::MutexGuard<RemoteSchedulerState> = state_m.lock().unwrap();
                state.ticked = true;
            } else if message[0] == 250 { // play
                let mut state: std::sync::MutexGuard<RemoteSchedulerState> = state_m.lock().unwrap();
                state.restarted = true;
            }
        }));
        
        Scheduler {
            remote_state,
            next_pos: MidiTime::zero(),
            _clock_source
        }
    }

    fn check_ticked (&self) -> bool {
        let mut state: std::sync::MutexGuard<RemoteSchedulerState> = self.remote_state.lock().unwrap();
        if state.ticked {
            state.ticked = false;
            true
        } else {
            false
        }
    }
}

impl Iterator for Scheduler {
    type Item = Option<ScheduleRange>;

    fn next(&mut self) -> Option<Self::Item> {
        thread::sleep(Duration::from_micros(10));

        let from = self.next_pos;
        let jumped = false;

        if self.check_ticked() {
            self.next_pos = from + MidiTime::tick();
            return Some(Some(ScheduleRange { 
                from, 
                to: self.next_pos, 
                ticked: true, 
                jumped 
            }))
            // } else if self.estimate_sub_tick_pos() >= from {
            //     self.next_pos = self.estimate_sub_tick_pos() + MidiTime::from_sub_ticks(1);
            //     return Some(Some(ScheduleRange { 
            //         from, 
            //         to: self.next_pos, 
            //         ticked: false, 
            //         jumped: false 
            //     }))
            // }
        }

        Some(None)
    }
}

#[derive(Debug, Copy, Clone)]
pub struct ScheduleRange {
    pub from: MidiTime,
    pub to: MidiTime,
    pub ticked: bool,
    pub jumped: bool
}

fn until (time: Instant) -> Duration {
    let now = Instant::now();
    if now < time {
        time.duration_since(now)
    } else {
        Duration::from_secs(0)
    }
}