use std::time::{Duration, SystemTime};
use std::thread;
use std::sync::mpsc;
use std::fmt::Debug;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct ClockSource<T> where T: Copy + Debug + Send + 'static {
    last_tap: SystemTime,
    channel: mpsc::Sender<T>,
    message: T,
    tempo: Arc<AtomicUsize>
}

impl<T> ClockSource<T> where T: Copy + Debug + Send + 'static {
    
    pub fn new (channel: mpsc::Sender<T>, message: T) -> ClockSource<T> {
        ClockSource {
            last_tap: SystemTime::now(),
            channel: channel,
            message: message,
            tempo: Arc::new(AtomicUsize::new(120))
        }
    }

    pub fn start (&self) {
        let mut last_time = SystemTime::now();
        let channel = self.channel.clone();
        let message = self.message;
        let tempo_ref = Arc::clone(&self.tempo);
    
        thread::spawn(move || {
            loop {
                let tempo = tempo_ref.load(Ordering::Relaxed);
                let tick_time = last_time + duration_from_float(1000.0 / (tempo as f64 / 60.0) / 24.0);
                channel.send(message).unwrap();
                if let Ok(duration) = tick_time.duration_since(last_time) {
                    thread::sleep(duration);
                }
                last_time = tick_time;
            }
        });
    }

    pub fn tap (&mut self) {
        let tap_time = SystemTime::now();
        let duration = tap_time.duration_since(self.last_tap).unwrap_or(Duration::from_secs(0));

        if duration < Duration::from_millis(1500) {
            let ms = duration_as_ms(duration);
            self.tempo.store(((60.0 / ms as f64) * 1000.0) as usize, Ordering::Relaxed);
        }

        self.last_tap = tap_time;
    }
}

fn duration_as_ms (duration: Duration) -> u32 {
    (duration.as_secs() * 1000 + duration.subsec_nanos() as u64 / 1_000_000) as u32
}

fn duration_from_float (float: f64) -> Duration {
    Duration::new(0, (float * 1_000_000.0) as u32) 
}