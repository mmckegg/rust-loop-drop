use ::serde::{Deserialize, Serialize};
use chunk::{Shape, Coords, ChunkMap, RepeatMode};
use serde_json::{json, to_writer_pretty};
use std::error::Error;
use std::fs::File;
use std::io::BufReader;

impl Config {
    pub fn read(filepath: &str) -> Result<Self, Box<Error>> {
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
        let tr6s_port_name = "TR-6S"; // drums
        let nts1_port_name = "NTS-1 digital kit"; // synth
        let streichfett_port_name = "Streichfett"; // synth
        let blackbox_output_name = "RK006 PORT 2"; // output 1
        let blackbox_output_name = "RK006 PORT 2"; // output 1
        let bluebox_output_name = "RK006 PORT 3"; // output 2
        let typhon_output_name = "RK006 PORT 4"; // output 3
        let fx_output_name = "RK006 PORT 6"; // output 5
        let cv1_output_name = "RK006 PORT 7"; // output 6
        let cv2_output_name = "RK006 PORT 9"; // output 8

        Config {
            chunks: vec![

                // EXT SYNTH
                ChunkConfig {
                    coords: Coords::new(0 + 8, 0),
                    shape: Shape::new(3, 8),
                    color: 125, // gross
                    channel: Some(2),
                    repeat_mode: RepeatMode::Global,
                    device: DeviceConfig::multi(vec![
                        DeviceConfig::MidiKeys {
                            output: MidiPortConfig::new(nts1_port_name, 1),
                            offset_id: String::from("ext"),
                            note_offset: -4,
                            octave_offset: -1
                        },
                        DeviceConfig::MidiKeys {
                            output: MidiPortConfig::new(blackbox_output_name, 2),
                            offset_id: String::from("ext"),
                            note_offset: -4,
                            octave_offset: -1
                        },
                        DeviceConfig::MidiTriggers {
                            output: MidiPortConfig::new(blackbox_output_name, 3),
                            trigger_ids: (36..60).collect(),
                            sidechain_output: None
                        }
                    ])
                },

                // EXT SYNTH OFFSET 
                // (also sends pitch mod on channel 2 for slicer)
                ChunkConfig {
                    coords: Coords::new(3 + 8, 0), 
                    shape: Shape::new(1, 8),
                    color: 12, // soft yellow
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                    device: DeviceConfig::multi(vec![
                        DeviceConfig::offset("ext"),
                        DeviceConfig::PitchOffsetChunk {
                            output: MidiPortConfig::new(blackbox_output_name, 3)
                        }
                    ])
                },

                // BASS OFFSET
                ChunkConfig {
                    device: DeviceConfig::offset("bass"), 
                    coords: Coords::new(4 + 8, 0), 
                    shape: Shape::new(1, 8),
                    color: 43, // blue
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant
                },

                // SYNTH OFFSET
                ChunkConfig {
                    device: DeviceConfig::offset("keys"), 
                    coords: Coords::new(5 + 8, 0), 
                    shape: Shape::new(1, 8),
                    color: 55, // pink
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant
                },
                
                // ROOT NOTE SELECTOR
                ChunkConfig {
                    device: DeviceConfig::RootSelect, 
                    coords: Coords::new(6 + 8, 0), 
                    shape: Shape::new(2, 8),
                    color: 35, // soft green
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant
                },

                // SCALE MODE SELECTOR
                ChunkConfig {
                    device: DeviceConfig::ScaleSelect, 
                    coords: Coords::new(16, 0), 
                    shape: Shape::new(1, 8),
                    color: 0, // black
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant
                },

                // DRUMS
                ChunkConfig {
                    device: DeviceConfig::MidiTriggers {
                        output: MidiPortConfig::new(tr6s_port_name, 10),
                        trigger_ids: vec![36, 38, 43, 39, 42, 46],
                        sidechain_output: Some(SidechainOutput {
                            id: 0,
                            port: MidiPortConfig::new(bluebox_output_name, 1),
                            trigger_id: 36
                        })
                    },
                    coords: Coords::new(0, 0), 
                    shape: Shape::new(1, 6),
                    color: 15, // yellow
                    channel: Some(0),
                    repeat_mode: RepeatMode::NoCycle
                },

                // EXTRA PERC (to fill in for only 6 triggers on cycles)
                ChunkConfig {
                    device: DeviceConfig::MidiTriggers {
                        output: MidiPortConfig::new(blackbox_output_name, 10),
                        trigger_ids: vec![40, 41],
                        sidechain_output: None
                    },
                    coords: Coords::new(0, 6), 
                    shape: Shape::new(1, 2),
                    color: 9, // orange
                    channel: Some(1),
                    repeat_mode: RepeatMode::NoCycle
                },

                // SAMPLER
                ChunkConfig {
                    device: DeviceConfig::MidiTriggers {
                        output: MidiPortConfig::new(blackbox_output_name, 10),
                        trigger_ids: vec![48, 49, 50, 51, 44, 45, 46, 47],
                        sidechain_output: None
                    },
                    coords: Coords::new(1, 0), 
                    shape: Shape::new(1, 8),
                    color: 9, // orange
                    channel: Some(1),
                    repeat_mode: RepeatMode::OnlyQuant
                },

                // BASS
                ChunkConfig {
                    device: DeviceConfig::MidiKeys {
                        output: MidiPortConfig::new(typhon_output_name, 1),
                        offset_id: String::from("bass"),
                        note_offset: -4,
                        octave_offset: -2
                    },
                    coords: Coords::new(2, 0), 
                    shape: Shape::new(3, 8),
                    color: 43, // blue
                    channel: Some(2),
                    repeat_mode: RepeatMode::Global
                },

                // SYNTH
                ChunkConfig {
                    device: DeviceConfig::MidiKeys {
                        output: MidiPortConfig::new(streichfett_port_name, 1),
                        offset_id: String::from("keys"),
                        note_offset: -4,
                        octave_offset: 0
                    },                    coords: Coords::new(5, 0), 
                    shape: Shape::new(3, 8),
                    color: 59, // pink
                    channel: Some(3),
                    repeat_mode: RepeatMode::Global
                }
            ],
            clock_input_port_name: String::from(tr6s_port_name),
            clock_output_port_names: vec![String::from(nts1_port_name)],
            resync_port_names: vec![String::from(blackbox_output_name)],
            keep_alive_port_names: vec![String::from(tr6s_port_name)],
            controllers: vec![
                ControllerConfig::Twister {
                    port_name: String::from("Midi Fighter Twister"),
                    mixer_port: MidiPortConfig::new(bluebox_output_name, 1),
                    modulators: vec![
                        ModulatorConfig::rx(tr6s_port_name, 1, Modulator::Cc(20, 64)),
                        ModulatorConfig::rx(tr6s_port_name, 1, Modulator::Cc(23, 64)),
                        ModulatorConfig::new(cv1_output_name, 1, Modulator::Cc(1, 64)),
                        ModulatorConfig::new(cv2_output_name, 1, Modulator::Cc(1, 64)),

                        ModulatorConfig::new(blackbox_output_name, 1, Modulator::Cc(1, 64)),
                        ModulatorConfig::new(blackbox_output_name, 1, Modulator::Cc(2, 64)),
                        ModulatorConfig::new(blackbox_output_name, 1, Modulator::Cc(3, 64)),
                        ModulatorConfig::new(blackbox_output_name, 1, Modulator::Cc(4, 64)),

                        ModulatorConfig::new(typhon_output_name, 1, Modulator::PitchBend(0.0)),
                        ModulatorConfig::new(typhon_output_name, 1, Modulator::Cc(4, 64)),
                        ModulatorConfig::rx(streichfett_port_name, 1, Modulator::PitchBend(0.0)),
                        ModulatorConfig::rx(streichfett_port_name, 1, Modulator::Cc(4, 0)),

                        ModulatorConfig::rx(nts1_port_name, 1, Modulator::PitchBend(0.0)),
                        ModulatorConfig::rx(nts1_port_name, 1, Modulator::Cc(43, 64)), 

                        ModulatorConfig::rx(fx_output_name, 1, Modulator::MaxCc(42, 14, 6)),
                        ModulatorConfig::rx(fx_output_name, 1, Modulator::Cc(5, 64)),
                    ]
                },
                ControllerConfig::Init {
                    modulators: vec![
                        // default patch for NTS-1
                        ModulatorConfig::new(nts1_port_name, 1, Modulator::Cc(53, 64)), // OSC TYPE: Souper
                        ModulatorConfig::new(nts1_port_name, 1, Modulator::Cc(54, 35)), // Detune
                        ModulatorConfig::new(nts1_port_name, 1, Modulator::Cc(88, 21)), // MOD TYPE: Chorus
                        ModulatorConfig::new(nts1_port_name, 1, Modulator::Cc(90, 36)), // REVERB TYPE: Room
                        ModulatorConfig::new(nts1_port_name, 1, Modulator::Cc(19, 64)), // Release                        
                        ModulatorConfig::rx(nts1_port_name, 1, Modulator::Cc(45, 65)), // Filter Env Time
                        ModulatorConfig::rx(nts1_port_name, 1, Modulator::Cc(46, 92)), // Filter Env Amt


                    ]
                },
                ControllerConfig::Umi3 {
                    port_name: String::from("Logidy UMI3")
                }
            ]
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
    pub controllers: Vec<ControllerConfig>
}

#[derive(Serialize, Deserialize)]
pub struct ChunkConfig {
    pub coords: Coords,
    pub shape: Shape,
    pub color: u8,
    pub channel: Option<u32>,
    pub repeat_mode: RepeatMode,
    pub device: DeviceConfig
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MidiPortConfig {
    pub name: String,
    pub channel: u8
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SidechainOutput {
    pub port: MidiPortConfig,
    pub trigger_id: u8,
    pub id: u32
}

impl MidiPortConfig {
    pub fn new (name: &str, channel: u8) -> Self {
        MidiPortConfig {
            name: String::from(name),
            channel
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum DeviceConfig {
    Multi {
        devices: Vec<DeviceConfig>
    },
    MidiKeys { 
        output: MidiPortConfig, 
        offset_id: String,
        note_offset: i32,
        octave_offset: i32
    },
    OffsetChunk { id: String },
    PitchOffsetChunk { output: MidiPortConfig },
    RootSelect,
    ScaleSelect,
    MidiTriggers { 
        output: MidiPortConfig,
        trigger_ids: Vec<u8>,
        sidechain_output: Option<SidechainOutput>
    }
}

impl DeviceConfig {
    pub fn offset (id: &str) -> Self {
        DeviceConfig::OffsetChunk {
            id: String::from(id)
        }
    }

    pub fn multi (devices: Vec<DeviceConfig>) -> Self {
        DeviceConfig::Multi {
            devices
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum ControllerConfig {
    Twister {
        port_name: String,
        mixer_port: MidiPortConfig,
        modulators: Vec<Option<ModulatorConfig>>
    },
    Umi3 {
        port_name: String
    },
    Init {
        modulators: Vec<Option<ModulatorConfig>>
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ModulatorConfig {
    pub port: MidiPortConfig,
    pub rx_port: Option<MidiPortConfig>,
    pub modulator: Modulator
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Modulator {
    Cc(u8, u8),
    MaxCc(u8, u8, u8),
    PitchBend(f64)
}

impl ModulatorConfig {
    pub fn new (port_name: &str, port_number: u8, modulator: Modulator) -> Option<Self> {
        Some(ModulatorConfig {
            port: MidiPortConfig::new(port_name, port_number),
            rx_port: None,
            modulator
        })
    }
    pub fn rx (port_name: &str, port_number: u8, modulator: Modulator) -> Option<Self> {
        Some(ModulatorConfig {
            port: MidiPortConfig::new(port_name, port_number),
            rx_port: Some(MidiPortConfig::new(port_name, port_number)),
            modulator
        })
    }
}