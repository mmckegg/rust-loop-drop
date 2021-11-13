use crate::devices::MidiTrigger;
use chunk::{Coords, RepeatMode, Shape};
use serde::{Deserialize, Serialize};
use serde_json::{json, to_writer_pretty};
use std::error::Error;
use std::fs::File;
use std::io::BufReader;

impl Config {
    pub fn read(filepath: &str) -> Result<Self, Box<dyn Error>> {
        let file = File::open(filepath)?;
        let reader = BufReader::new(file);

        let config = serde_json::from_reader(reader)?;
        Ok(config)
    }

    pub fn write(&self, filepath: &str) -> std::io::Result<()> {
        let myjson = json!(self);
        // println!("{}", myjson.to_string());
        to_writer_pretty(&File::create(filepath)?, &myjson)?;
        Ok(())
    }

    pub fn default() -> Self {
        let poly_port_name = "POLY2"; // output 3
        let launchpad_midi_output_name = "Launchpad Pro MK3 PORT 2"; // output 3

        Config {
            chunks: vec![
                // EXT SYNTH OFFSET
                // (also sends pitch mod on channel 2 for slicer)
                ChunkConfig {
                    coords: Coords::new(3 + 8, 0),
                    shape: Shape::new(1, 8),
                    color: 12, // soft yellow
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                    device: DeviceConfig::multi(vec![DeviceConfig::offset("ext")]),
                },
                // BASS OFFSET
                ChunkConfig {
                    device: DeviceConfig::offset("bass"),
                    coords: Coords::new(4 + 8, 0),
                    shape: Shape::new(1, 8),
                    color: 43, // blue
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // SYNTH OFFSET
                ChunkConfig {
                    device: DeviceConfig::offset("keys"),
                    coords: Coords::new(5 + 8, 0),
                    shape: Shape::new(1, 8),
                    color: 55, // pink
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // ROOT NOTE SELECTOR
                ChunkConfig {
                    device: DeviceConfig::RootSelect {
                        output_modulators: vec![],
                    },
                    coords: Coords::new(6 + 8, 0),
                    shape: Shape::new(2, 8),
                    color: 35, // soft green
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // SCALE MODE SELECTOR
                ChunkConfig {
                    device: DeviceConfig::ScaleSelect,
                    coords: Coords::new(16, 0),
                    shape: Shape::new(1, 8),
                    color: 0, // black
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // DRUMS
                ChunkConfig {
                    device: DeviceConfig::CcTriggers {
                        output: MidiPortConfig::new(poly_port_name, 1),
                        triggers: vec![
                            MidiTrigger::Note(1, 0, 127),  // kick (g1)
                            MidiTrigger::Cc(1, 1, 127),    // clap (1)
                            MidiTrigger::Note(2, 0, 127),  // hat (g2)
                            MidiTrigger::Cc(1, 3, 127),    // cym a (3)
                            MidiTrigger::Note(1, 64, 127), // step (12)
                            MidiTrigger::Cc(1, 2, 127),    // tick (2)
                            MidiTrigger::Note(2, 20, 127), // open hat (g2p2)
                            MidiTrigger::Cc(1, 4, 127),    // cym b (4)
                        ],
                    },
                    coords: Coords::new(0, 0),
                    shape: Shape::new(2, 4),
                    color: 15, // yellow
                    channel: Some(0),
                    repeat_mode: RepeatMode::NoCycle,
                },
                ChunkConfig {
                    device: DeviceConfig::MidiTriggers {
                        output: MidiPortConfig::new(poly_port_name, 10),
                        velocity_map: Some(vec![80, 80, 127]),
                        trigger_ids: vec![36, 38, 40, 41],
                        sidechain_output: Some(SidechainOutput { id: 0 }),
                    },
                    coords: Coords::new(0, 4),
                    shape: Shape::new(1, 4),
                    color: 8, // warm white
                    channel: Some(0),
                    repeat_mode: RepeatMode::NoCycle,
                },
                // SAMPLER
                ChunkConfig {
                    device: DeviceConfig::MidiTriggers {
                        output: MidiPortConfig::new(poly_port_name, 10),
                        velocity_map: Some(vec![100, 127]),
                        trigger_ids: vec![48, 49, 50, 51, 44, 45, 46, 47],
                        sidechain_output: None,
                    },
                    coords: Coords::new(1, 4),
                    shape: Shape::new(1, 4),
                    color: 9, // orange
                    channel: Some(2),
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // BASSLINE
                ChunkConfig {
                    device: DeviceConfig::MidiKeys {
                        output: MidiPortConfig::new(poly_port_name, 3),
                        velocity_map: None,
                        offset_id: String::from("bass"),
                        note_offset: -4,
                        octave_offset: -2,
                    },
                    coords: Coords::new(2, 0),
                    shape: Shape::new(6, 4),
                    color: 43, // blue
                    channel: Some(4),
                    repeat_mode: RepeatMode::Global,
                },
                // PLAITS
                ChunkConfig {
                    device: DeviceConfig::MidiKeys {
                        output: MidiPortConfig::new(poly_port_name, 4),
                        velocity_map: None,
                        offset_id: String::from("keys"),
                        note_offset: -4,
                        octave_offset: -2,
                    },
                    coords: Coords::new(2, 4),
                    shape: Shape::new(6, 4),
                    color: 59, // pink
                    channel: Some(5),
                    repeat_mode: RepeatMode::Global,
                },
                // POLY CINEMATIC
                ChunkConfig {
                    coords: Coords::new(0 + 8, 0),
                    shape: Shape::new(3, 8),
                    color: 125, // gross
                    channel: Some(6),
                    repeat_mode: RepeatMode::Global,
                    device: DeviceConfig::multi(vec![DeviceConfig::MidiKeys {
                        output: MidiPortConfig::new(launchpad_midi_output_name, 5),
                        velocity_map: None,
                        offset_id: String::from("ext"),
                        note_offset: -4,
                        octave_offset: -1,
                    }]),
                },
            ],
            clock_input_port_name: String::from("Launchpad Pro MK3"),
            clock_output_port_names: vec![String::from(poly_port_name)],
            resync_port_names: vec![],
            keep_alive_port_names: vec![],
            controllers: vec![
                ControllerConfig::Umi3 {
                    port_name: String::from("Logidy UMI3"),
                },
                ControllerConfig::ModTwister {
                    port_name: String::from("Midi Fighter Twister"),
                    modulators: vec![
                        ModulatorConfig::new(poly_port_name, 1, Modulator::Cc(5, 64)), // drum filter (5)
                        ModulatorConfig::new(poly_port_name, 1, Modulator::MaxCc(6, 64, 0)), // bass filter (6)
                        ModulatorConfig::new(poly_port_name, 1, Modulator::Cc(7, 64)), // plaits filter (7)
                        ModulatorConfig::new(poly_port_name, 1, Modulator::MaxCc(8, 64, 0)), // poly filter (8)
                        ModulatorConfig::new(poly_port_name, 1, Modulator::PitchBend(0.0)), // kick pitch (p1)
                        ModulatorConfig::new(poly_port_name, 3, Modulator::PitchBend(0.0)), // bass pitch (p3)
                        ModulatorConfig::new(poly_port_name, 4, Modulator::PitchBend(0.0)), // plaits pitch (p4)
                        ModulatorConfig::new(poly_port_name, 5, Modulator::PitchBend(0.0)), // poly pitch (midi)
                        ModulatorConfig::new(poly_port_name, 2, Modulator::PositivePitchBend(0.0)), // hihat mod (p2)
                        ModulatorConfig::new(poly_port_name, 1, Modulator::MaxCc(10, 64, 0)), // mod a (10)
                        ModulatorConfig::new(poly_port_name, 1, Modulator::MaxCc(11, 64, 0)), // mod b (11)
                        ModulatorConfig::new(poly_port_name, 1, Modulator::Cc(9, 64)), // bus filter (12)
                    ],
                },
                ControllerConfig::LaunchpadTempo {
                    daw_port_name: String::from("Launchpad Pro MK3 PORT 3"),
                },
            ],
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub chunks: Vec<ChunkConfig>,
    pub clock_input_port_name: String,
    pub clock_output_port_names: Vec<String>,
    pub keep_alive_port_names: Vec<String>,
    pub resync_port_names: Vec<String>,
    pub controllers: Vec<ControllerConfig>,
}

#[derive(Serialize, Deserialize)]
pub struct ChunkConfig {
    pub coords: Coords,
    pub shape: Shape,
    pub color: u8,
    pub channel: Option<u32>,
    pub repeat_mode: RepeatMode,
    pub device: DeviceConfig,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MidiPortConfig {
    pub name: String,
    pub channel: u8,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SidechainOutput {
    pub id: u32,
}

impl MidiPortConfig {
    pub fn new(name: &str, channel: u8) -> Self {
        MidiPortConfig {
            name: String::from(name),
            channel,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum DeviceConfig {
    Multi {
        devices: Vec<DeviceConfig>,
    },
    MidiKeys {
        output: MidiPortConfig,
        offset_id: String,
        note_offset: i32,
        velocity_map: Option<Vec<u8>>,
        octave_offset: i32,
    },
    OffsetChunk {
        id: String,
    },
    PitchOffsetChunk {
        output: MidiPortConfig,
    },
    RootSelect {
        output_modulators: Vec<Option<ModulatorConfig>>,
    },
    ScaleSelect,
    MidiTriggers {
        output: MidiPortConfig,
        trigger_ids: Vec<u8>,
        velocity_map: Option<Vec<u8>>,
        sidechain_output: Option<SidechainOutput>,
    },
    CcTriggers {
        output: MidiPortConfig,
        triggers: Vec<MidiTrigger>,
    },
}

impl DeviceConfig {
    pub fn offset(id: &str) -> Self {
        DeviceConfig::OffsetChunk {
            id: String::from(id),
        }
    }

    pub fn multi(devices: Vec<DeviceConfig>) -> Self {
        DeviceConfig::Multi { devices }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum ControllerConfig {
    Twister {
        port_name: String,
        mixer_port: MidiPortConfig,
        modulators: Vec<Option<ModulatorConfig>>,
    },
    ModTwister {
        port_name: String,
        modulators: Vec<Option<ModulatorConfig>>,
    },
    Umi3 {
        port_name: String,
    },
    VT4Key {
        output: MidiPortConfig,
    },
    ClockPulse {
        output: MidiPortConfig,
        divider: i32,
    },
    LaunchpadTempo {
        daw_port_name: String,
    },
    Init {
        modulators: Vec<Option<ModulatorConfig>>,
    },
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ModulatorConfig {
    pub port: MidiPortConfig,
    pub rx_port: Option<MidiPortConfig>,
    pub modulator: Modulator,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Modulator {
    Cc(u8, u8),
    MaxCc(u8, u8, u8),
    PolarCcSwitch {
        cc_low: Option<u8>,
        cc_high: Option<u8>,
        cc_switch: Option<u8>,
        default: u8,
    },
    PitchBend(f64),
    PositivePitchBend(f64),
}

impl ModulatorConfig {
    pub fn new(port_name: &str, port_number: u8, modulator: Modulator) -> Option<Self> {
        Some(ModulatorConfig {
            port: MidiPortConfig::new(port_name, port_number),
            rx_port: None,
            modulator,
        })
    }
    pub fn rx(port_name: &str, port_number: u8, modulator: Modulator) -> Option<Self> {
        Some(ModulatorConfig {
            port: MidiPortConfig::new(port_name, port_number),
            rx_port: Some(MidiPortConfig::new(port_name, port_number)),
            modulator,
        })
    }
}
