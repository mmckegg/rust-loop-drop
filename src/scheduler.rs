extern crate circular_queue;
use self::circular_queue::CircularQueue;

use std::time::{Duration, Instant};
use std::thread;
use midi_connection;
use std::sync::{Arc, Mutex};


pub use ::midi_time::MidiTime;

pub struct Scheduler {
    state: Arc<Mutex<SchedulerState>>,
    block_duration: Duration,
    last_tick: Option<u64>,
    started: bool,
    last_tick_at: Instant,
    pos: MidiTime,
    _clock_source: midi_connection::ThreadReference
}

impl Scheduler {
    pub fn start (clock_port_name: &str, block_duration: Duration) -> Self {
        let state = Arc::new(Mutex::new(SchedulerState {
            tick_duration: Duration::from_millis(500),
            tick: None
        }));

        // track external clock and tick durations (to calculate bpm)
        let state_m = state.clone();
        let mut last_tick_durations: CircularQueue<u64> = CircularQueue::with_capacity(12);
        let mut last_stamp = None;
        let _clock_source = midi_connection::get_input(clock_port_name, move |stamp, message| {
            if message[0] == 248 {
                let mut state: std::sync::MutexGuard<SchedulerState> = state_m.lock().unwrap();
                if let Some(last_stamp) = last_stamp {
                    let duration = stamp - last_stamp;

                    // assume stopped clock if duration is more than 2 seconds
                    if duration < 2000000 {
                        last_tick_durations.push(duration)
                    }
                }

                last_stamp = Some(stamp);
                if let Some(tick) = state.tick {
                    state.tick = Some(tick + 1);
                } else {
                    state.tick = Some(0)
                }

                if last_tick_durations.len() > 0 {
                    state.tick_duration = Duration::from_micros(last_tick_durations.iter().sum::<u64>() / (last_tick_durations.len() as u64));
                    println!("duration {}", state.tick_duration.as_secs_f64());
                }
            } else if message[0] == 250 { // play
                // todo: handle re-quantize!
            }
        });
        
        Scheduler {
            state,
            started: false,
            block_duration,
            last_tick: None,
            last_tick_at: Instant::now(),
            pos: MidiTime::zero(),
            _clock_source
        }
    }
}

impl Iterator for Scheduler {
    type Item = Schedule;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            thread::sleep(self.block_duration);

            let state: std::sync::MutexGuard<SchedulerState> = self.state.lock().unwrap();
            let now = Instant::now();

            // wait until we have a tick
            if let Some(tick) = state.tick {
                let delta = (now + self.block_duration) - self.last_tick_at;
                let frac = delta.as_secs_f64() / state.tick_duration.as_secs_f64();
                
                if state.tick != self.last_tick {
                    self.last_tick_at = now;
                    self.last_tick = state.tick;
                }
                
                if frac < 1.0 {
                    let from = self.pos;
                    let to = MidiTime::from_float(tick as f64 + frac);
                    if to > from {
                        self.pos = to;
                        return Some(Schedule { from, to })
                    }
                }

            }
        }
    }
}

pub struct Schedule {
    pub from: MidiTime,
    pub to: MidiTime
}

struct SchedulerState {
    tick: Option<u64>,
    tick_duration: Duration
}