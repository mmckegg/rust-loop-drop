extern crate bus;

pub use self::bus::{Bus, BusReader};
pub use ::midi_time::MidiTime;

use std::time::{Duration, Instant};
use std::thread;
use std::sync::mpsc;
use std::sync::{Arc};
use std::sync::atomic::{AtomicUsize, Ordering};
use ::midi_connection;

const DEFAULT_TEMPO: usize = 120;

pub struct ClockSource {
    bus: Bus<FromClock>,
    tx: mpsc::Sender<ToClock>,
    broadcast: mpsc::Sender<ClockMessage>,
    to_broadcast: mpsc::Receiver<ClockMessage>,
    tempo: Arc<AtomicUsize>,
    _midi_input: midi_connection::ThreadReference,
    midi_outputs: Vec<midi_connection::SharedMidiOutputConnection>,
    internal_clock_suppressed_to: Instant,
    tick_pos: MidiTime
}

impl ClockSource {
    
    pub fn new (midi_port_name: &str, output_ports: Vec<midi_connection::SharedMidiOutputConnection>) -> ClockSource {
        let tempo = Arc::new(AtomicUsize::new(DEFAULT_TEMPO));
        let tempo_ref = Arc::clone(&tempo);

        let (tx, rx) = mpsc::channel();
        let (broadcast, to_broadcast) = mpsc::channel();

        let tx_feedback = tx.clone();
        let broadcast_rx = broadcast.clone();
        let broadcast_external = broadcast.clone();

        let external_input = midi_connection::get_input(midi_port_name, move |_stamp, message| {
            if message[0] == 248 {
                broadcast_external.send(ClockMessage::ExternalTick).unwrap();
            } else if message[0] == 250 {
                broadcast_external.send(ClockMessage::ExternalPlay).unwrap();
            }
        });

        thread::spawn(move || {
            let mut last_tap = Instant::now();
            for msg in rx {
                match msg {
                    ToClock::SetTempo(value) => {
                        tempo_ref.store(value, Ordering::Relaxed);
                        broadcast_rx.send(ClockMessage::Tempo(value)).unwrap();
                    },
                    ToClock::TapTempo => {
                        let tap_time = Instant::now();
                        let duration = tap_time.duration_since(last_tap);

                        if duration < Duration::from_millis(1500) {
                            let ms = duration_as_ms(duration);
                            let value = ((60.0 / ms as f64) * 1000.0) as usize;
                            tx_feedback.send(ToClock::SetTempo(value)).unwrap();
                        }

                        last_tap = tap_time;
                    },
                    ToClock::Nudge(offset) => {
                        broadcast_rx.send(ClockMessage::Nudge(offset)).unwrap();
                    }
                }
            }
        });

        ClockSource {
            bus: Bus::new(10),
            internal_clock_suppressed_to: Instant::now(),
            _midi_input: external_input,
            midi_outputs: output_ports,
            tick_pos: MidiTime::zero(),
            tx,
            tempo,
            broadcast,
            to_broadcast
        }
    }

    pub fn start (&mut self) {
        let tempo_ref = Arc::clone(&self.tempo);
        let broadcast_clock = self.broadcast.clone();

        thread::spawn(move || {
            let mut last_changed_at = Instant::now();
            let mut ticks_at_last_changed = 0;
            let mut last_tempo = 120;
            let mut ticks = 0;
            loop {
                let tempo = tempo_ref.load(Ordering::Relaxed);

                if tempo != last_tempo {
                    last_changed_at = Instant::now();
                    ticks_at_last_changed = ticks;
                    last_tempo = tempo;
                }

                broadcast_clock.send(ClockMessage::InternalTick).unwrap();
                ticks += 1;

                let ticks_since_last_change = ticks - ticks_at_last_changed;
                let beat_duration = 60.0 / last_tempo as f64;
                let tick_duration = beat_duration / 24.0;
                let from_last_change_until_next_tick = duration_from_float(ticks_since_last_change as f64 * tick_duration);
                let since_last_change = last_changed_at.elapsed();
                if from_last_change_until_next_tick > since_last_change {
                    thread::sleep(from_last_change_until_next_tick - since_last_change);
                }
            }
        });

        self.bus.broadcast(FromClock::Tempo(DEFAULT_TEMPO));

        for msg in &self.to_broadcast {
            match msg {
                ClockMessage::InternalTick => {
                    if self.internal_clock_suppressed_to < Instant::now() {
                        self.bus.broadcast(FromClock::Schedule {
                            pos: self.tick_pos, 
                            length: MidiTime::tick()
                        });
                        self.tick_pos = self.tick_pos + MidiTime::tick();
                        for port in &mut self.midi_outputs {
                            port.send(&[248]).unwrap();
                        }
                    }
                },
                ClockMessage::ExternalTick => {
                    self.internal_clock_suppressed_to = Instant::now() + Duration::new(0, 500 * 1_000_000);
                    self.bus.broadcast(FromClock::Schedule {
                        pos: self.tick_pos, 
                        length: MidiTime::tick()
                    });
                    for port in &mut self.midi_outputs {
                        port.send(&[248]).unwrap();
                    }
                    self.tick_pos = self.tick_pos + MidiTime::tick();
                },
                ClockMessage::ExternalPlay => {
                    let offset = self.tick_pos % MidiTime::from_beats(1);
                    if offset >= MidiTime::from_ticks(12) {
                        self.tick_pos = self.tick_pos + (MidiTime::from_beats(1) - offset);
                    } else {
                        self.tick_pos = self.tick_pos - offset;
                    }
                    //self.midi_output.send(&[250]).unwrap();
                    self.bus.broadcast(FromClock::Jump);
                },
                ClockMessage::Tempo(value) => {
                    self.bus.broadcast(FromClock::Tempo(value));
                },
                ClockMessage::Nudge(offset) => {
                    self.tick_pos = self.tick_pos + offset;
                    self.bus.broadcast(FromClock::Jump);
                }
            }
        }
    }

    pub fn sync_clock_start (&mut self, output: midi_connection::SharedMidiOutputConnection) {
        let clock = self.add_rx();
        // pipe clock in
        thread::spawn(move || {
            let mut output = output;
            for msg in clock.receiver {
                match msg {
                    FromClock::Schedule {pos, ..} => {
                        if pos % MidiTime::from_beats(32) == MidiTime::zero() {
                            output.send(&[242, 0, 0]).unwrap();
                        }
                    },
                    _ => ()
                }
            }
        });
    }

    pub fn add_rx (&mut self) -> RemoteClock {
        RemoteClock {
            receiver: self.bus.add_rx(),
            sender: self.tx.clone()
        }
    }
}

fn duration_as_ms (duration: Duration) -> u32 {
    (duration.as_secs() * 1000 + duration.subsec_nanos() as u64 / 1_000_000) as u32
}

fn duration_from_float (float: f64) -> Duration {
    Duration::new(float as u64, ((float % 1.0) * 1_000_000_000.0) as u32) 
}

#[derive(Clone, Debug)]
pub enum FromClock {
    Schedule { 
        pos: MidiTime, 
        length: MidiTime 
    },
    Tempo(usize),
    Jump
}

#[derive(Debug)]
pub enum ToClock {
    TapTempo,
    SetTempo(usize),
    Nudge(MidiTime)
}

enum ClockMessage {
    Tempo(usize),
    Nudge(MidiTime),
    InternalTick,
    ExternalTick,
    ExternalPlay
}

pub struct RemoteClock {
    pub sender: mpsc::Sender<ToClock>,
    pub receiver: BusReader<FromClock>
}
