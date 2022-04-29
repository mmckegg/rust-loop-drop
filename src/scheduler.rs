extern crate circular_queue;
use self::circular_queue::CircularQueue;

use midi_connection;
use std::sync::atomic::{AtomicBool, Ordering};
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
    use_internal_clock: Arc<AtomicBool>,
    tx_int_tick: mpsc::SyncSender<Option<Duration>>,
    remote_state: Arc<Mutex<RemoteSchedulerState>>,
    _clock_source: Option<midi_connection::ThreadReference>,
}

struct RemoteSchedulerState {
    tick_durations: CircularQueue<Duration>,
    last_tick_stamp: Option<u64>,
    tick_start_at: Instant,
    clock_source: ClockSource,
    stamp_offset: u64,
    jumped: bool,
    started: bool,
    last_tick_at: Option<Instant>,
}
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
enum ClockSource {
    External,
    Internal,
    PendingInternal,
    PendingExternal,
}

impl RemoteSchedulerState {
    fn restart(&mut self, offset: u64) {
        self.stamp_offset = offset;
        self.last_tick_stamp = None;
        self.tick_start_at = Instant::now();
        self.started = true;
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
    pub fn start(clock_port_name: &str, use_internal_clock: Arc<AtomicBool>) -> Self {
        let remote_state = Arc::new(Mutex::new(RemoteSchedulerState {
            tick_durations: CircularQueue::with_capacity(3),
            last_tick_at: None,
            started: true,
            last_tick_stamp: None,
            jumped: false,
            clock_source: ClockSource::External,
            tick_start_at: Instant::now(),
            stamp_offset: 0,
        }));

        let (tx, rx) = mpsc::sync_channel(8);
        let (tx_int_tick, rx_int_tick) = mpsc::sync_channel(8);
        let (tx_sub_tick, rx_sub_tick) = mpsc::sync_channel(8);
        let tx_clock = tx.clone();

        let tx_sub_tick_int = tx_sub_tick.clone();
        let tx_int_tick_ext = tx_int_tick.clone();

        // track external clock and tick durations (to calculate bpm)
        let state_m = remote_state.clone();
        let _clock_source = Some(midi_connection::get_input(
            clock_port_name,
            move |stamp, message| {
                if message[0] == 248 {
                    if let Ok(mut state) = state_m.try_lock() {
                        if !state.started || state.clock_source == ClockSource::Internal {
                            return;
                        }

                        if state.clock_source == ClockSource::PendingExternal {
                            tx_int_tick_ext.send(None).unwrap();
                            state.clock_source = ClockSource::External;
                        }

                        if state.clock_source == ClockSource::External {
                            let restarted = state.jumped;
                            state.jumped = false;

                            state.tick(stamp);
                            tx_clock.send(ScheduleTick::MidiTick(restarted)).unwrap();

                            if SUB_TICKS > 1 {
                                if tx_sub_tick
                                    .try_send((
                                        SUB_TICKS - 1,
                                        state.tick_duration() / SUB_TICKS as u32,
                                    ))
                                    .is_err()
                                {
                                    println!("[WARN] Can't send subticks");
                                }
                            }
                        } else if state.clock_source == ClockSource::PendingInternal {
                            state.clock_source = ClockSource::Internal;
                            tx_int_tick_ext.send(Some(state.tick_duration())).unwrap();
                        }
                    } else {
                        println!("[WARN] External tick can't acquire state lock");
                    }
                } else if message[0] == 250 {
                    // // play
                    // println!("restart clock");
                    // let mut state: std::sync::MutexGuard<RemoteSchedulerState> =
                    //     state_m.lock().unwrap();
                    // state.restart(stamp);
                    // state.jumped = true;
                }
            },
        ));

        let tx_sub_clock = tx.clone();
        thread::spawn(move || {
            for (tick_count, duration) in rx_sub_tick {
                for i in 0..tick_count {
                    thread::sleep(duration);
                    tx_sub_clock.send(ScheduleTick::SubTick(i + 1)).unwrap();
                }
            }
        });

        let tx_int_clock = tx.clone();
        thread::spawn(move || {
            let mut tick_duration = None;
            let mut next_tick_at = Instant::now();
            loop {
                if tick_duration.is_some() {
                    // check to see if the tick needs updating, or cancelling
                    if let Ok(update) = rx_int_tick.try_recv() {
                        next_tick_at = Instant::now();
                        tick_duration = update;
                    }
                } else {
                    // block until request to switch to internal clock received
                    tick_duration = rx_int_tick.recv().unwrap();
                    next_tick_at = Instant::now();
                }

                if let Some(tick_duration) = tick_duration {
                    next_tick_at += tick_duration;
                    tx_int_clock.send(ScheduleTick::MidiTick(false)).unwrap();
                    if SUB_TICKS > 1 {
                        tx_sub_tick_int
                            .send((SUB_TICKS - 1, tick_duration / SUB_TICKS as u32))
                            .unwrap();
                    }

                    // sleep until next tick
                    if next_tick_at > Instant::now() {
                        thread::sleep(next_tick_at - Instant::now());
                    }
                }
            }
        });

        Scheduler {
            ticks: -1,
            sub_ticks: 0,
            rx,
            tx_int_tick,
            use_internal_clock,
            last_tick_at: Instant::now(),
            next_pos: MidiTime::zero(),
            remote_state,
            _clock_source,
        }
    }

    fn tick_duration(&self) -> Duration {
        let remote_state = self.remote_state.lock().unwrap();
        remote_state.tick_duration()
    }

    fn clock_source(&self) -> ClockSource {
        let remote_state = self.remote_state.lock().unwrap();
        remote_state.clock_source
    }

    fn switch_to_internal(&mut self, sync_external: bool) {
        if let Ok(mut remote_state) = self.remote_state.try_lock() {
            if sync_external {
                // wait for next external tick before switching to internal
                if remote_state.clock_source != ClockSource::Internal {
                    remote_state.clock_source = ClockSource::PendingInternal
                }
            } else {
                // start clock immediately
                remote_state.clock_source = ClockSource::Internal;
                self.tx_int_tick
                    .send(Some(remote_state.tick_duration()))
                    .unwrap();
            }
        } else {
            println!("[WARN] Can't acquire lock for switch to internal");
        }
    }

    fn switch_to_external(&mut self) {
        if let Ok(mut remote_state) = self.remote_state.try_lock() {
            if remote_state.clock_source != ClockSource::External {
                remote_state.clock_source = ClockSource::PendingExternal
            }
        } else {
            println!("[WARN] Can't acquire lock for switch to external");
        }
    }

    fn await_next(&mut self) -> ScheduleRange {
        loop {
            let msg = self.rx.recv_timeout(Duration::from_millis(200));
            let mut from = self.next_pos;
            let use_internal_clock = self.use_internal_clock.load(Ordering::Relaxed);
            let clock_source = self.clock_source();

            if use_internal_clock && clock_source == ClockSource::External {
                // switch to internal clock on the next external tick (unless timeouts out)
                self.switch_to_internal(true)
            } else if !use_internal_clock && clock_source == ClockSource::Internal {
                self.switch_to_external()
            }

            match msg {
                Err(_err) => {
                    // force a tick to trigger if the external clock has stopped
                    println!("[INFO] Fallback to internal clock");
                    self.switch_to_internal(false)
                }
                Ok(ScheduleTick::MidiTick(jumped)) => {
                    self.last_tick_at = Instant::now();
                    self.sub_ticks = 0;

                    if jumped {
                        self.ticks = self.ticks / 768 * 768 + 768 + 1;
                        from = MidiTime::new(self.ticks, 0);
                        self.next_pos = MidiTime::new(self.ticks, 1);
                    } else {
                        self.ticks += 1;
                        self.next_pos = MidiTime::new(self.ticks, 1);
                    }
                    return ScheduleRange {
                        from,
                        to: self.next_pos,
                        tick_pos: MidiTime::from_ticks(self.ticks),
                        ticked: true,
                        jumped,
                    };
                }
                Ok(ScheduleTick::SubTick(sub_tick)) => {
                    if sub_tick == self.sub_ticks + 1 {
                        self.sub_ticks = sub_tick;
                        self.next_pos = MidiTime::new(self.ticks, self.sub_ticks + 1);
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
    MidiTick(bool),
    SubTick(u8),
}
