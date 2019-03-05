use std::process::{Command, Child};
use std::sync::Arc;
use std::thread;
use std::sync::Mutex;
use std::time::{SystemTime, Duration};
use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::collections::HashMap;
use std::os::unix::net::UnixListener;
use std::net::Shutdown;
use std::fs;

extern crate humantime;


pub struct LastTrigger (Option<SystemTime>);
pub struct Children (Option<ChildGuard>);

impl LastTrigger {

    fn trigger (&mut self) {
        self.0 = Some(SystemTime::now());
    }

    fn since_trigger (&self) -> Option<Duration> {
        match self.0 {
            Some(time) => Some(time.elapsed().unwrap()),
            None => None
        }
    }
}

impl Children {
    fn stop (&mut self) {
        self.0 = None
    }

    fn set (&mut self, child: Child) {
        self.0 = Some(ChildGuard(child))
    }

    fn is_running (&mut self) -> bool {
        if let Some(ref mut child) = self.0 {
            child.0.try_wait().unwrap().is_none()
        } else {
            false
        }
    }
}

pub struct AudioRecorder {
    started_at: Arc<Mutex<SystemTime>>,
    children: Arc<Mutex<Children>>,
    last_trigger: Arc<Mutex<LastTrigger>>,
    meta_output: Arc<Mutex<Option<File>>>,
    last_tempo: Arc<AtomicUsize>,
    last_channel_volumes: Arc<Mutex<HashMap<u32, u8>>>,
    failed_at: Option<SystemTime>,
    pub tx: mpsc::Sender<AudioRecorderEvent>
}

#[derive(Debug)]
pub enum AudioRecorderEvent {
    Tempo(usize),
    ChannelVolume(u32, u8),
    Tick
}

impl AudioRecorder {
    pub fn new () -> AudioRecorder {
        let last_trigger = Arc::new(Mutex::new(LastTrigger(None)));
        let children = Arc::new(Mutex::new(Children(None)));
        let last_tempo = Arc::new(AtomicUsize::new(120));
        let last_channel_volumes = Arc::new(Mutex::new(HashMap::new()));
        let meta_output: Arc<Mutex<Option<File>>> = Arc::new(Mutex::new(None));
        let started_at = Arc::new(Mutex::new(SystemTime::now()));

        let (tx, rx) = mpsc::channel();

        let children_s = children.clone();
        let meta_output_s = meta_output.clone();

        let last_trigger_c = last_trigger.clone();
        let children_c = children.clone();
        let meta_output_c = meta_output.clone();
        let last_tempo_loop = last_tempo.clone();
        let last_channel_volumes_loop = last_channel_volumes.clone();
        let meta_output_loop = meta_output.clone();
        let started_at_loop = started_at.clone();

        thread::spawn(move || {
            let socket_path = "/tmp/stop-recording";
            fs::remove_file(socket_path).ok();
            let listener = UnixListener::bind(socket_path).unwrap();

            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        children_s.lock().unwrap().stop();
                        let mut output_mutex = meta_output_s.lock().unwrap();
                        *output_mutex = None;
                        println!("Stopping recording due to signal on /tmp/restart-recording");
                        stream.shutdown(Shutdown::Both);
                    }
                    _ => ()
                }                
            }
        });

        thread::spawn(move || {
            for msg in rx {
                if let Some(ref mut output) = *meta_output_loop.lock().unwrap() {
                    let started_at = started_at_loop.lock().unwrap();
                    log_to_file(output, &started_at, &msg);

                    // store last value
                    match msg {
                        AudioRecorderEvent::Tempo(value) => {
                            last_tempo_loop.store(value, Ordering::Relaxed);
                        },
                        AudioRecorderEvent::ChannelVolume(channel, value) => {
                            last_channel_volumes_loop.lock().unwrap().insert(channel, value);
                        },
                        _ => ()
                    }
                }
            }
        });

        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_secs(1));

                // ensure that event log is flushed every 1 second
                if let Some(ref mut output) = *meta_output_c.lock().unwrap() {
                    output.flush().unwrap();
                }

                if let Some(since) = last_trigger_c.lock().unwrap().since_trigger() {
                    if since > Duration::from_secs(60) {
                        children_c.lock().unwrap().stop();
                        let mut output_mutex = meta_output_c.lock().unwrap();
                        *output_mutex = None;
                    }
                }
            }
        });

        AudioRecorder {
            children,
            started_at,
            last_trigger,
            meta_output,
            last_tempo,
            failed_at: None,
            last_channel_volumes,
            tx
        }
    }

    pub fn trigger (&mut self) {
        let mut should_start = false;
        self.last_trigger.lock().unwrap().trigger();
        {
            let mut children = self.children.lock().unwrap();
            if !children.is_running() {
                children.stop();
                should_start = true;
            }
        }

        if should_start {
            self.start();
        }
    }

    pub fn is_recording (&self) -> bool {
        let mut children = self.children.lock().unwrap();
        children.is_running()
    }

    pub fn start (&mut self) {
        if let Some(failed_at) = self.failed_at {
            if failed_at.elapsed().unwrap() < Duration::from_secs(5) {
                return // don't start if failed recently (less than 5 seconds ago)!
            }
        } 
        let name = format!("recording-{}", humantime::format_rfc3339_seconds(SystemTime::now())).replace(":", "");
        let output_path = format!("/media/usb/{}.wav", &name);

        if let Ok(mut meta_file) = File::create(&format!("{}.events", &output_path)) {
            let child = Command::new("/usr/bin/arecord").args(&[
                "--channels", "8",
                "--device", "plughw:CARD=KMix,DEV=0",
                "--format", "S16_LE",
                "--rate", "44100",
                &output_path
            ]).spawn().unwrap();

            let started_at = SystemTime::now();

            log_to_file(&mut meta_file, &started_at, 
                &AudioRecorderEvent::Tempo(self.last_tempo.load(Ordering::Relaxed))
            );

            for (channel, value) in self.last_channel_volumes.lock().unwrap().iter() {
                log_to_file(&mut meta_file, &started_at, 
                    &AudioRecorderEvent::ChannelVolume(*channel, *value)
                );
            }

            meta_file.flush().unwrap();
            
            let mut meta_output_mutex = self.meta_output.lock().unwrap();
            *meta_output_mutex = Some(meta_file);

            let mut started_at_mutex = self.started_at.lock().unwrap();
            *started_at_mutex = started_at;

            self.children.lock().unwrap().set(child);
        } else {
            self.failed_at = Some(SystemTime::now());
            println!("could not spawn recorder (missing usb drive?)");
        }
        
    }
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        // You can check std::thread::panicking() here
        match self.0.kill() {
            Err(e) => println!("Could not kill child process: {}", e),
            Ok(_) => println!("Successfully killed child process"),
        }
    }
}

pub fn log_to_file (file: &mut File, started_at: &SystemTime, event: &AudioRecorderEvent) -> bool {
    let stamp = started_at.elapsed().unwrap();
    match event {
        &AudioRecorderEvent::Tick => {
            write!(file, "[{}.{:06}, \"tick\"]\n", stamp.as_secs(), stamp.subsec_micros())
        },
        &AudioRecorderEvent::Tempo(value) => {
            write!(file, "[{}.{:06}, \"tempo\", {}]\n", stamp.as_secs(), stamp.subsec_micros(), value)
        },
        &AudioRecorderEvent::ChannelVolume(channel, value) => {
            write!(file, "[{}.{:06}, \"channel_volume\", {}, {}]\n", stamp.as_secs(), stamp.subsec_micros(), channel, value)
        }
    }.is_ok()
}