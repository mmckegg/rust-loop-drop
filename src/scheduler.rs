extern crate circular_queue;
use self::circular_queue::CircularQueue;

use std::time::{Duration, Instant};
use std::thread;
use midi_connection;
use std::sync::{Arc, Mutex};


pub use ::midi_time::{MidiTime, SUB_TICKS};

pub struct Scheduler {
    remote_state: Arc<Mutex<RemoteSchedulerState>>,
    tick_stamps: CircularQueue<(i32, Instant)>,
    sync_delta: MidiTime,
    last_tick_at: Instant,
    start_offset: MidiTime,
    next_pos: MidiTime,
    _clock_source: Option<midi_connection::ThreadReference>
}

struct RemoteSchedulerState {
    ticks: Option<i32>,
    tick_stamps: CircularQueue<(i32, Instant)>,
    tick_durations: CircularQueue<Duration>,
    last_tick_stamp: Option<u64>,
    tick_start_at: Instant,
    stamp_offset: u64,
    pending_restart: bool,
    started: bool,
    last_tick_at: Option<Instant>
}

impl RemoteSchedulerState {
    fn restart (&mut self, offset: u64) {
        self.started = true;
        self.ticks = None;
        self.last_tick_stamp = None;
        self.stamp_offset = offset;
        self.tick_start_at = Instant::now();
        self.last_tick_at = None;
    }


    fn tick (&mut self, stamp: u64) {
        self.ticks = if let Some(ticks) = self.ticks {
            Some(ticks + 1)
        } else {
            self.pending_restart = true;
            Some(0)
        };

        
        if let Some(ticks) = self.ticks {
            if let Some(last_tick_stamp) = self.last_tick_stamp {
                let duration = Duration::from_micros(stamp - last_tick_stamp);
                if duration < Duration::from_millis(500) {
                    self.tick_durations.push(Duration::from_micros(stamp - last_tick_stamp));
                }
            }
            self.tick_stamps.push((ticks, self.tick_start_at + Duration::from_micros(stamp - self.stamp_offset)));
        }

        self.last_tick_stamp = Some(stamp);
        self.last_tick_at = Some(Instant::now());
    }
}

impl Scheduler {
    pub fn start (clock_port_name: &str) -> Self {
        let remote_state = Arc::new(Mutex::new(RemoteSchedulerState {
            tick_stamps: CircularQueue::with_capacity(12),
            tick_durations: CircularQueue::with_capacity(3),
            last_tick_at: None,
            ticks: None,
            started: false,
            pending_restart: false,
            last_tick_stamp: None,
            tick_start_at: Instant::now(),
            stamp_offset: 0
        }));

        // track external clock and tick durations (to calculate bpm)
        let state_m = remote_state.clone();
        let _clock_source = Some(midi_connection::get_input(clock_port_name, move |stamp, message| {
            
            if message[0] == 248 {
                let mut state: std::sync::MutexGuard<RemoteSchedulerState> = state_m.lock().unwrap();

                // if we get a tick before clock start, treat as clock start
                if !state.started {
                    state.restart(stamp);
                }

                state.tick(stamp);
            } else if message[0] == 250 { // play
                let mut state: std::sync::MutexGuard<RemoteSchedulerState> = state_m.lock().unwrap();
                state.restart(stamp);
            }
        }));
        
        Scheduler {
            remote_state,
            tick_stamps: CircularQueue::with_capacity(12),
            sync_delta: MidiTime::zero(),
            last_tick_at: Instant::now(),
            next_pos: MidiTime::zero(),
            start_offset: MidiTime::zero(),
            _clock_source
        }
    }

    fn tick_duration (&self) -> Option<Duration> {
        let state: std::sync::MutexGuard<RemoteSchedulerState> = self.remote_state.lock().unwrap();
        let sum = state.tick_durations.iter().sum::<Duration>();
        let count = state.tick_durations.len() as u32;
        if count > 1 {
            let average = sum.as_secs_f64() / count as f64;
            let drift_multipler = self.tick_drift_multipler().max(0.5);
            Some(Duration::from_secs_f64(average * drift_multipler))
        } else {
            None
        }
    }

    fn tick_drift_multipler (&self) -> f64 {
        let delta = self.sync_delta.as_float();
        let curved = (delta * delta) / 2.0;
        if delta > 0.0 {
            1.0 + curved
        } else {
            1.0 - curved
        }
    }
 
    fn default_tick_duration (&self) -> Duration {
        Duration::from_secs_f64(0.5 / 24.0)
    }

    fn next_sub_tick_at (&self) -> Instant {
        let duration = self.tick_duration().unwrap_or(self.default_tick_duration());
        let sub_tick_duration = duration / SUB_TICKS as u32;
        let sub_ticks = if self.next_pos.sub_ticks() == 0 {
            SUB_TICKS
        } else {
            self.next_pos.sub_ticks()
        };

        let delta = sub_tick_duration * sub_ticks as u32;
        self.last_tick_at + delta
    }

    fn get_pending_restart (&mut self) -> bool {
        let mut state: std::sync::MutexGuard<RemoteSchedulerState> = self.remote_state.lock().unwrap();
        if state.pending_restart && state.ticks.is_some() {
            state.pending_restart = false;
            true
        } else {
            false
        }
    }

    fn restart (&mut self) {
        let resync_grid = MidiTime::from_beats(8);
        let rounded_pos = self.next_pos.round();
        let offset = rounded_pos % resync_grid;
        if offset >= resync_grid / 2 {
            self.next_pos = rounded_pos.round() + (resync_grid - offset);
        } else {
            self.next_pos = rounded_pos.round() - offset;
        }
        self.start_offset = self.next_pos;
    }

    fn calibrate_to_external (&mut self)  {
        if let Some(tick_duration) = self.tick_duration() {
            let state: std::sync::MutexGuard<RemoteSchedulerState> = self.remote_state.lock().unwrap();
            if let Some(ticks) = state.ticks {
                if let Some(last_tick_at) = state.last_tick_at {
                    let since: Duration = last_tick_at.elapsed();
                    if since < tick_duration {
                        let half_point = tick_duration / 2;
    
                        let resolved_ticks = if since < half_point {
                            ticks // we just had it
                        } else {
                            ticks + 1 // we're gonna have it soon so add 1!
                        };
    
                        let reference_pos = self.next_pos - self.start_offset;
                        let remote_pos = MidiTime::from_ticks(resolved_ticks);
                        self.sync_delta = reference_pos - remote_pos;
                    }
                }
            }
        }
    }
}

impl Iterator for Scheduler {
    type Item = ScheduleRange;

    fn next(&mut self) -> Option<Self::Item> {
        let next_sub_tick_at = self.next_sub_tick_at(); 
        thread::sleep(until(next_sub_tick_at));

        let jumped = self.get_pending_restart();
        if jumped {
            self.restart();
        }

        let ticked = self.next_pos.sub_ticks() == 0;

        if ticked {
            self.calibrate_to_external();
        }

        let from = self.next_pos;
        let ticks = from.ticks();
        let sub_ticks = from.sub_ticks();

        let to = self.next_pos.floor() + MidiTime::from_sub_ticks(sub_ticks + 1);
        
        self.next_pos = to;

        if ticked {
            self.last_tick_at = next_sub_tick_at;
            self.tick_stamps.push((ticks, self.last_tick_at));
        }

        return Some(ScheduleRange { from, to, ticked, jumped })
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