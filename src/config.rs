use serde::{Deserialize, Serialize};
use serde_json::Result;

#[derive(Serialize, Deserialize)]
struct Config {
    chunks: Vec<ChunkConfig>,
    clock_input_port_name: String,
    clock_output_port_names: Vec<String>,
    resync_port_names: Vec<String>
}

#[derive(Serialize, Deserialize)]
struct ChunkConfig {
    // Choords
    x: u32,
    y: u32,
    // Shape
    height: u32,
    width: u32,
    color: u8,
    channel: Option<u32>,
    repeat_mode: RepeatMode
}

#[derive(Serialize, Deserialize)]
struct MidiPortConfig {
    name: String,
    channel: u8
}

enum DeviceConfig {
  MultiChunk,
  MidiKeys { 
      outputs: Vec<MidiPortConfig>, 
      offset_id: Option<String>,
      note_offset: i32,
      octave_offset: i32
  },
  BlackboxSlicer { output: MidiPortConfig },
  OffsetChunk { id: String },
  PitchOffsetChunk { output: MidiPortConfig },
  RootSelect,
  ScaleSelect,
  TR6s { output: MidiPortConfig },
  BlackboxPerc { output: MidiPortConfig },
  BlackboxSample { output: MidiPortConfig }
}

