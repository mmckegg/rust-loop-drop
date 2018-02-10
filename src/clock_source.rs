extern crate bus;

pub use self::bus::{Bus, BusReader};
pub use ::midi_time::MidiTime;

use std::time::{Duration, SystemTime};
use std::thread;
use std::sync::mpsc;
use std::fmt::Debug;
use std::sync::{Arc, Weak};
use std::sync::atomic::{AtomicUsize, Ordering};
use ::midi_connection;

const DEFAULT_TEMPO: usize = 120;

pub struct ClockSource {
    bus: Bus<FromClock>,
    tx: mpsc::Sender<ToClock>,
    broadcast: mpsc::Sender<ClockMessage>,
    to_broadcast: mpsc::Receiver<ClockMessage>,
    tempo: Arc<AtomicUsize>,
    midi_input: midi_connection::MidiInputConnection<()>,
    midi_output: midi_connection::MidiOutputConnection,
    internal_clock_suppressed_to: SystemTime,
    tick_pos: MidiTime
}

impl ClockSource {
    
    pub fn new (midi_port_name: &str) -> ClockSource {
        let tempo = Arc::new(AtomicUsize::new(DEFAULT_TEMPO));
        let tempo_ref = Arc::clone(&tempo);

        let (tx, rx) = mpsc::channel();
        let (broadcast, to_broadcast) = mpsc::channel();

        let tx_feedback = tx.clone();
        let broadcast_rx = broadcast.clone();
        let broadcast_external = broadcast.clone();

        let external_input = midi_connection::get_input(midi_port_name, move |_stamp, message, _| {
            if message[0] == 248 {
                broadcast_external.send(ClockMessage::ExternalTick);
            } else if message[0] == 250 {
                broadcast_external.send(ClockMessage::ExternalPlay);
            }
        }, ()).unwrap();

        let external_output = midi_connection::get_output(midi_port_name).unwrap();

        thread::spawn(move || {
            let mut last_tap = SystemTime::now();
            for msg in rx {
                match msg {
                    ToClock::SetTempo(value) => {
                        tempo_ref.store(value, Ordering::Relaxed);
                        broadcast_rx.send(ClockMessage::Tempo(value));
                    },
                    ToClock::TapTempo => {
                        let tap_time = SystemTime::now();
                        let duration = tap_time.duration_since(last_tap).unwrap_or(Duration::from_secs(0));

                        if duration < Duration::from_millis(1500) {
                            let ms = duration_as_ms(duration);
                            let value = ((60.0 / ms as f64) * 1000.0) as usize;
                            tx_feedback.send(ToClock::SetTempo(value));
                        }

                        last_tap = tap_time;
                    },
                    ToClock::Nudge(offset) => {
                        broadcast_rx.send(ClockMessage::Nudge(offset));
                    }
                }
            }
        });

        ClockSource {
            bus: Bus::new(10),
            internal_clock_suppressed_to: SystemTime::now(),
            midi_input: external_input,
            midi_output: external_output,
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
            let mut last_time = SystemTime::now();
            loop {
                let tempo = tempo_ref.load(Ordering::Relaxed);
                let tick_time = last_time + duration_from_float(1000.0 / (tempo as f64 / 60.0) / 24.0);

                broadcast_clock.send(ClockMessage::InternalTick).unwrap();

                if let Ok(duration) = tick_time.duration_since(last_time) {
                    thread::sleep(duration);
                }
                last_time = tick_time;
            }
        });

        self.bus.broadcast(FromClock::Tempo(DEFAULT_TEMPO));

        for msg in &self.to_broadcast {
            match msg {
                ClockMessage::InternalTick => {
                    if self.internal_clock_suppressed_to < SystemTime::now() {
                        self.bus.broadcast(FromClock::Schedule {
                            pos: self.tick_pos, 
                            length: MidiTime::tick()
                        });
                        self.tick_pos = self.tick_pos + MidiTime::tick();
                        self.midi_output.send(&[248]).unwrap();
                    }
                },
                ClockMessage::ExternalTick => {
                    self.internal_clock_suppressed_to = SystemTime::now() + Duration::new(0, 500 * 1_000_000);
                    self.bus.broadcast(FromClock::Schedule {
                        pos: self.tick_pos, 
                        length: MidiTime::tick()
                    });
                    self.midi_output.send(&[248]).unwrap();
                    self.tick_pos = self.tick_pos + MidiTime::tick();
                },
                ClockMessage::ExternalPlay => {
                    let offset = self.tick_pos % MidiTime::from_beats(1);
                    if offset >= MidiTime::from_ticks(12) {
                        self.tick_pos = self.tick_pos + (MidiTime::from_beats(1) - self.tick_pos);
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
    Duration::new(0, (float * 1_000_000.0) as u32) 
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
