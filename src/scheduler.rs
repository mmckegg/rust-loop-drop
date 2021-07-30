extern crate circular_queue;
use self::circular_queue::CircularQueue;

use midi_connection;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub use midi_time::{MidiTime, SUB_TICKS};

pub struct Scheduler {
    next_pos: MidiTime,
    last_tick_at: Instant,
    ticks: i32,
    sub_ticks: u8,
    rx: mpsc::Receiver<ScheduleTick>,
    _clock_source: Option<midi_connection::ThreadReference>,
}

struct RemoteSchedulerState {
    tick_durations: CircularQueue<Duration>,
    last_tick_stamp: Option<u64>,
    tick_start_at: Instant,
    stamp_offset: u64,
    started: bool,
    last_tick_at: Option<Instant>,
}

impl RemoteSchedulerState {
    fn restart(&mut self, offset: u64) {
        self.stamp_offset = offset;
        self.last_tick_stamp = None;
        self.tick_start_at = Instant::now();
    }

    fn tick_duration(&self) -> Duration {
        let sum = self.tick_durations.iter().sum::<Duration>();
        let count = self.tick_durations.len() as u32;
        if count > 1 {
            let average = sum.as_secs_f64() / count as f64;
            Duration::from_secs_f64(average)
        } else {
            Duration::from_secs_f64(0.5 / 24.0)
        }
    }

    fn tick(&mut self, stamp: u64) {
        if let Some(last_tick_stamp) = self.last_tick_stamp {
            let duration = Duration::from_micros(stamp - last_tick_stamp);
            let d = duration.as_millis();

            if duration < Duration::from_millis(500) {
                self.tick_durations
                    .push(Duration::from_micros(stamp - last_tick_stamp));
                self.last_tick_at =
                    Some(self.tick_start_at + Duration::from_micros(stamp - self.stamp_offset));
            } else {
                self.restart(stamp);
            }
        }

        self.last_tick_stamp = Some(stamp);
    }
}

impl Scheduler {
    pub fn start(clock_port_name: &str) -> Self {
        let remote_state = Arc::new(Mutex::new(RemoteSchedulerState {
            tick_durations: CircularQueue::with_capacity(3),
            last_tick_at: None,
            started: false,
            last_tick_stamp: None,
            tick_start_at: Instant::now(),
            stamp_offset: 0,
        }));

        let (tx, rx) = mpsc::sync_channel(8);
        let tx_clock = tx.clone();

        // track external clock and tick durations (to calculate bpm)
        let state_m = remote_state.clone();
        let _clock_source = Some(midi_connection::get_input(
            clock_port_name,
            move |stamp, message| {
                if message[0] == 248 {
                    let mut state: std::sync::MutexGuard<RemoteSchedulerState> =
                        state_m.lock().unwrap();

                    // if we get a tick before clock start, treat as clock start
                    if !state.started {
                        state.restart(stamp);
                    }

                    state.tick(stamp);
                    tx_clock.send(ScheduleTick::MidiTick).unwrap();
                } else if message[0] == 250 {
                    // play
                    let mut state: std::sync::MutexGuard<RemoteSchedulerState> =
                        state_m.lock().unwrap();
                    state.restart(stamp);
                }
            },
        ));

        let state_s = remote_state.clone();
        let tx_sub_clock = tx.clone();
        // thread::spawn(move || loop {
        //     let state: std::sync::MutexGuard<RemoteSchedulerState> = state_s.lock().unwrap();
        //     let duration = state.tick_duration() / (SUB_TICKS as u32);
        //     drop(state);
        //     thread::sleep(duration);
        //     tx_sub_clock.send(ScheduleTick::SubTick(duration)).unwrap();
        // });

        Scheduler {
            ticks: -1,
            sub_ticks: 0,
            rx,
            last_tick_at: Instant::now(),
            next_pos: MidiTime::zero(),
            _clock_source,
        }
    }

    fn await_next(&mut self) -> ScheduleRange {
        loop {
            let msg = self.rx.recv().unwrap();
            let from = self.next_pos;

            match msg {
                ScheduleTick::MidiTick => {
                    self.last_tick_at = Instant::now();
                    self.sub_ticks = 0;
                    self.ticks += 1;
                    self.next_pos = MidiTime::new(self.ticks, self.sub_ticks);

                    return ScheduleRange {
                        from,
                        to: self.next_pos,
                        tick_pos: MidiTime::from_ticks(self.ticks),
                        ticked: true,
                        jumped: false,
                    };
                }
                ScheduleTick::SubTick(duration) => {
                    if from.sub_ticks() < (SUB_TICKS - 1)
                        && self.last_tick_at.elapsed() > (duration / 2)
                    {
                        self.sub_ticks += 1;
                        self.next_pos = MidiTime::new(self.ticks, self.sub_ticks);
                        return ScheduleRange {
                            from,
                            to: self.next_pos,
                            tick_pos: MidiTime::from_ticks(self.ticks),
                            ticked: false,
                            jumped: false,
                        };
                    }
                }
            };
        }
    }
}

impl Iterator for Scheduler {
    type Item = ScheduleRange;

    fn next(&mut self) -> Option<Self::Item> {
        Some(self.await_next())
    }
}

#[derive(Debug, Copy, Clone)]
pub struct ScheduleRange {
    pub from: MidiTime,
    pub to: MidiTime,
    pub tick_pos: MidiTime,
    pub ticked: bool,
    pub jumped: bool,
}

enum ScheduleTick {
    MidiTick,
    SubTick(Duration),
}
