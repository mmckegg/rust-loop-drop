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
        to_writer_pretty(&File::create(filepath)?, &myjson)?;
        Ok(())
    }

    pub fn default() -> Self {
        let rig_port_name = "Univer Inter";

        Config {
            clock_input_port_name: None,
            clock_output_port_names: vec![rig_port_name.to_string()],
            resync_port_names: vec![],
            keep_alive_port_names: vec![],
            samples: SampleRackConfig {
                output: MidiPortConfig::new(rig_port_name, 10),
                notes: [40, 41, 42, 43, 36, 37, 38, 39],
                volume_ccs: [11, 12, 13, 14, 15, 16, 17, 18],
                color: ChunkColor::Yellow,
            },
            triggers: TriggerRackConfig {
                output: MidiPortConfig::new(rig_port_name, 3),
                notes: [36, 37, 38, 39, 40, 41, 42, 43],
                color: ChunkColor::Orange,
            },
            voice_a: VoiceConfig {
                output: MidiPortConfig::new(rig_port_name, 15),
                note_offset: -4,
                octave_offset: -1,
                monophonic: true,
                offset_wrap: false,
                color: ChunkColor::Blue,
            },
            voice_b: VoiceConfig {
                output: MidiPortConfig::new(rig_port_name, 14),
                note_offset: -4,
                octave_offset: -2,
                monophonic: true,
                offset_wrap: false,
                color: ChunkColor::Pink,
            },
            voice_c: VoiceConfig {
                output: MidiPortConfig::new(rig_port_name, 7),
                note_offset: -4,
                octave_offset: -1,
                monophonic: false,
                offset_wrap: true,
                color: ChunkColor::Purple,
            },
            modulation: ModulationSurfaceConfig {
                tap_ms: 200,
                double_tap_ms: 350,
                hold_ms: 350,
                encoders: vec![
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 1, col: 5 },
                        label: String::from("Voice A Bend"),
                        color: EncoderColor::Blue,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::VoiceA],
                        assignment: EncoderAssignment::PitchBend {
                            output: MidiPortConfig::new(rig_port_name, 15),
                            bipolar: true,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 1, col: 6 },
                        label: String::from("Voice B Bend"),
                        color: EncoderColor::Pink,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::VoiceB],
                        assignment: EncoderAssignment::PitchBend {
                            output: MidiPortConfig::new(rig_port_name, 14),
                            bipolar: true,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 1, col: 7 },
                        label: String::from("Voice C Bend"),
                        color: EncoderColor::Purple,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::VoiceC],
                        assignment: EncoderAssignment::PitchBend {
                            output: MidiPortConfig::new(rig_port_name, 7),
                            bipolar: true,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 1, col: 8 },
                        label: String::from("Root Note"),
                        color: EncoderColor::White,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![],
                        assignment: EncoderAssignment::RootNote { default: 64 },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 2, col: 1 },
                        label: String::from("Decay 1"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::Sample(0)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 31,
                            default: 50,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 2, col: 2 },
                        label: String::from("Decay 2"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::Sample(1)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 32,
                            default: 50,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 2, col: 3 },
                        label: String::from("Decay 3"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::Sample(2)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 33,
                            default: 50,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 2, col: 4 },
                        label: String::from("Decay 4"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::Sample(3)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 34,
                            default: 50,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 2, col: 5 },
                        label: String::from("Filter 1"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::Sample(4)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 37,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 2, col: 6 },
                        label: String::from("Filter 2"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::Sample(5)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 38,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 2, col: 7 },
                        label: String::from("Filter 3"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::Sample(6)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 39,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 2, col: 8 },
                        label: String::from("Filter 4"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::Sample(7)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 40,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 3, col: 1 },
                        label: String::from("Pitch 1"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::Sample(0)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 21,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 3, col: 2 },
                        label: String::from("Pitch 2"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::Sample(1)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 22,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 3, col: 3 },
                        label: String::from("Pitch 3"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::Sample(2)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 23,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 3, col: 4 },
                        label: String::from("Pitch 4"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::Sample(3)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 24,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 3, col: 5 },
                        label: String::from("Pitch 5"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::Sample(4)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 25,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 3, col: 6 },
                        label: String::from("Pitch 6"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::Sample(5)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 26,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 3, col: 7 },
                        label: String::from("Pitch 7"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::Sample(6)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 27,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Fixed { row: 3, col: 8 },
                        label: String::from("Pitch 8"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::Sample(7)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 28,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::A,
                            row: 1,
                            col: 1,
                        },
                        label: String::from("Gran 5 Time"),
                        color: EncoderColor::Yellow,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::Sample(4)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 45,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::A,
                            row: 1,
                            col: 2,
                        },
                        label: String::from("Gran 5 Start"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::Sample(4)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 35,
                            default: 0,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::A,
                            row: 1,
                            col: 3,
                        },
                        label: String::from("Gran 6 Time"),
                        color: EncoderColor::Yellow,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::Sample(5)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 46,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::A,
                            row: 1,
                            col: 4,
                        },
                        label: String::from("Gran 6 Start"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::Sample(5)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 36,
                            default: 0,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::A,
                            row: 2,
                            col: 1,
                        },
                        label: String::from("CV 1"),
                        color: EncoderColor::Cyan,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 2),
                            cc: 20,
                            default: 0,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::A,
                            row: 2,
                            col: 2,
                        },
                        label: String::from("CV 2"),
                        color: EncoderColor::Cyan,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 2),
                            cc: 21,
                            default: 0,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::A,
                            row: 2,
                            col: 3,
                        },
                        label: String::from("LFO Speed"),
                        color: EncoderColor::Purple,
                        lfo_mode: LfoMode::None,
                        activity_highlights: vec![],
                        assignment: EncoderAssignment::LfoSpeed { default: 50 },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::A,
                            row: 2,
                            col: 4,
                        },
                        label: String::from("LFO Wave"),
                        color: EncoderColor::Purple,
                        lfo_mode: LfoMode::None,
                        activity_highlights: vec![],
                        assignment: EncoderAssignment::LfoWave { default: 64 },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::B,
                            row: 1,
                            col: 1,
                        },
                        label: String::from("Expr 1"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::Sample(0)],
                        assignment: EncoderAssignment::SampleLevelMultiplier {
                            sample: 0,
                            default: 90,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::B,
                            row: 1,
                            col: 2,
                        },
                        label: String::from("Expr 2"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::Sample(1)],
                        assignment: EncoderAssignment::SampleLevelMultiplier {
                            sample: 1,
                            default: 90,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::B,
                            row: 1,
                            col: 3,
                        },
                        label: String::from("Expr 3"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::Sample(2)],
                        assignment: EncoderAssignment::SampleLevelMultiplier {
                            sample: 2,
                            default: 90,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::B,
                            row: 1,
                            col: 4,
                        },
                        label: String::from("Expr 4"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::Sample(3)],
                        assignment: EncoderAssignment::SampleLevelMultiplier {
                            sample: 3,
                            default: 90,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::B,
                            row: 2,
                            col: 1,
                        },
                        label: String::from("Expr 5"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::Sample(4)],
                        assignment: EncoderAssignment::SampleLevelMultiplier {
                            sample: 4,
                            default: 90,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::B,
                            row: 2,
                            col: 2,
                        },
                        label: String::from("Expr 6"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::Sample(5)],
                        assignment: EncoderAssignment::SampleLevelMultiplier {
                            sample: 5,
                            default: 90,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::B,
                            row: 2,
                            col: 3,
                        },
                        label: String::from("Expr 7"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::Sample(6)],
                        assignment: EncoderAssignment::SampleLevelMultiplier {
                            sample: 6,
                            default: 90,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::B,
                            row: 2,
                            col: 4,
                        },
                        label: String::from("Expr 8"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::Sample(7)],
                        assignment: EncoderAssignment::SampleLevelMultiplier {
                            sample: 7,
                            default: 90,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::C,
                            row: 1,
                            col: 1,
                        },
                        label: String::from("Send A1"),
                        color: EncoderColor::Blue,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::VoiceA],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 11),
                            cc: 43,
                            default: 0,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::C,
                            row: 1,
                            col: 2,
                        },
                        label: String::from("Send A2"),
                        color: EncoderColor::Pink,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::VoiceB],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 11),
                            cc: 44,
                            default: 0,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::C,
                            row: 1,
                            col: 3,
                        },
                        label: String::from("Send A3"),
                        color: EncoderColor::Yellow,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::SampleRange(0, 3)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 11),
                            cc: 45,
                            default: 0,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::C,
                            row: 1,
                            col: 4,
                        },
                        label: String::from("Send A4"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::SampleRange(4, 7)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 11),
                            cc: 46,
                            default: 0,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::C,
                            row: 2,
                            col: 1,
                        },
                        label: String::from("Send B1"),
                        color: EncoderColor::Blue,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::VoiceA],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 11),
                            cc: 53,
                            default: 0,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::C,
                            row: 2,
                            col: 2,
                        },
                        label: String::from("Send B2"),
                        color: EncoderColor::Pink,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::VoiceB],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 11),
                            cc: 54,
                            default: 0,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::C,
                            row: 2,
                            col: 3,
                        },
                        label: String::from("Send B3"),
                        color: EncoderColor::Yellow,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::SampleRange(0, 3)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 11),
                            cc: 55,
                            default: 0,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::C,
                            row: 2,
                            col: 4,
                        },
                        label: String::from("Send B4"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![ActivityHighlight::SampleRange(4, 7)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 11),
                            cc: 56,
                            default: 0,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::D,
                            row: 1,
                            col: 1,
                        },
                        label: String::from("Pan 1"),
                        color: EncoderColor::Red,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::VoiceA],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 11),
                            cc: 73,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::D,
                            row: 1,
                            col: 2,
                        },
                        label: String::from("Pan 2"),
                        color: EncoderColor::Red,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::VoiceB],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 11),
                            cc: 74,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::D,
                            row: 1,
                            col: 3,
                        },
                        label: String::from("Pan 3"),
                        color: EncoderColor::Red,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::SampleRange(0, 3)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 11),
                            cc: 75,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::D,
                            row: 1,
                            col: 4,
                        },
                        label: String::from("Pan 4"),
                        color: EncoderColor::Red,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::SampleRange(4, 7)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 11),
                            cc: 76,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::D,
                            row: 2,
                            col: 1,
                        },
                        label: String::from("Reverb Time"),
                        color: EncoderColor::Pink,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 11),
                            cc: 12,
                            default: 0,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::D,
                            row: 2,
                            col: 2,
                        },
                        label: String::from("Delay Time"),
                        color: EncoderColor::Pink,
                        lfo_mode: LfoMode::UnipolarMultiply,
                        activity_highlights: vec![],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 11),
                            cc: 13,
                            default: 0,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::D,
                            row: 2,
                            col: 3,
                        },
                        label: String::from("Tone"),
                        color: EncoderColor::Pink,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 11),
                            cc: 14,
                            default: 64,
                        },
                    },
                    EncoderConfig {
                        slot: EncoderSlot::Banked {
                            bank: BankId::D,
                            row: 2,
                            col: 4,
                        },
                        label: String::from("DJ Filter"),
                        color: EncoderColor::Orange,
                        lfo_mode: LfoMode::BipolarOffset,
                        activity_highlights: vec![ActivityHighlight::SampleRange(0, 3)],
                        assignment: EncoderAssignment::MidiCc {
                            output: MidiPortConfig::new(rig_port_name, 10),
                            cc: 51,
                            default: 64,
                        },
                    },
                ],
            },
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub clock_input_port_name: Option<String>,
    pub clock_output_port_names: Vec<String>,
    pub keep_alive_port_names: Vec<String>,
    pub resync_port_names: Vec<String>,
    pub samples: SampleRackConfig,
    pub triggers: TriggerRackConfig,
    pub voice_a: VoiceConfig,
    pub voice_b: VoiceConfig,
    pub voice_c: VoiceConfig,
    pub modulation: ModulationSurfaceConfig,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SampleRackConfig {
    pub output: MidiPortConfig,
    pub notes: [u8; 8],
    pub volume_ccs: [u8; 8],
    pub color: ChunkColor,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TriggerRackConfig {
    pub output: MidiPortConfig,
    pub notes: [u8; 8],
    pub color: ChunkColor,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct VoiceConfig {
    pub output: MidiPortConfig,
    pub note_offset: i32,
    pub octave_offset: i32,
    pub monophonic: bool,
    pub offset_wrap: bool,
    pub color: ChunkColor,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ModulationSurfaceConfig {
    pub tap_ms: u64,
    pub double_tap_ms: u64,
    pub hold_ms: u64,
    pub encoders: Vec<EncoderConfig>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct EncoderConfig {
    pub slot: EncoderSlot,
    pub label: String,
    #[serde(default)]
    pub color: EncoderColor,
    #[serde(default)]
    pub lfo_mode: LfoMode,
    #[serde(default)]
    pub activity_highlights: Vec<ActivityHighlight>,
    pub assignment: EncoderAssignment,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum EncoderSlot {
    Fixed { row: u8, col: u8 },
    Banked { bank: BankId, row: u8, col: u8 },
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BankId {
    A,
    B,
    C,
    D,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum EncoderColor {
    White,
    Yellow,
    Orange,
    Blue,
    Purple,
    Pink,
    Cyan,
    Lime,
    Red,
    Green,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum LfoMode {
    None,
    UnipolarMultiply,
    BipolarOffset,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum ActivityHighlight {
    Sample(u8),
    SampleRange(u8, u8),
    Samples,
    VoiceA,
    VoiceB,
    VoiceC,
}

impl Default for LfoMode {
    fn default() -> Self {
        LfoMode::None
    }
}

impl Default for EncoderColor {
    fn default() -> Self {
        EncoderColor::White
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum EncoderAssignment {
    None,
    MidiCc {
        output: MidiPortConfig,
        cc: u8,
        default: u8,
    },
    InvertMidiCc {
        output: MidiPortConfig,
        cc: u8,
        default: u8,
    },
    MaxMidiCc {
        output: MidiPortConfig,
        cc: u8,
        max: u8,
        default: u8,
    },
    InvertMaxMidiCc {
        output: MidiPortConfig,
        cc: u8,
        max: u8,
        default: u8,
    },
    PolarCcSwitch {
        output: MidiPortConfig,
        cc_low: Option<u8>,
        cc_high: Option<u8>,
        cc_switch: Option<u8>,
        default: u8,
    },
    PitchBend {
        output: MidiPortConfig,
        bipolar: bool,
        default: u8,
    },
    Aftertouch {
        output: MidiPortConfig,
        default: u8,
    },
    LfoSpeed {
        default: u8,
    },
    LfoWave {
        default: u8,
    },
    RootNote {
        default: u8,
    },
    SampleLevelMultiplier {
        sample: u8,
        default: u8,
    },
}

impl EncoderAssignment {
    pub fn default_value(&self) -> u8 {
        match self {
            EncoderAssignment::None => 0,
            EncoderAssignment::MidiCc { default, .. }
            | EncoderAssignment::InvertMidiCc { default, .. }
            | EncoderAssignment::MaxMidiCc { default, .. }
            | EncoderAssignment::InvertMaxMidiCc { default, .. }
            | EncoderAssignment::PolarCcSwitch { default, .. }
            | EncoderAssignment::PitchBend { default, .. }
            | EncoderAssignment::Aftertouch { default, .. }
            | EncoderAssignment::LfoSpeed { default }
            | EncoderAssignment::LfoWave { default }
            | EncoderAssignment::RootNote { default }
            | EncoderAssignment::SampleLevelMultiplier { default, .. } => *default,
        }
    }

    pub fn supports_lfo_lane(&self) -> bool {
        match self {
            EncoderAssignment::None
            | EncoderAssignment::LfoSpeed { .. }
            | EncoderAssignment::LfoWave { .. }
            | EncoderAssignment::RootNote { .. } => false,
            _ => true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum ChunkColor {
    Yellow,
    Orange,
    Blue,
    Purple,
    Pink,
    Cyan,
    Lime,
}

impl ChunkColor {
    pub fn to_midi(self) -> u8 {
        match self {
            ChunkColor::Yellow => 19,
            ChunkColor::Orange => 10,
            ChunkColor::Blue => 75,
            ChunkColor::Purple => 97,
            ChunkColor::Pink => 122,
            ChunkColor::Cyan => 39,
            ChunkColor::Lime => 37,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MidiPortConfig {
    pub name: String,
    pub channel: u8,
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
pub enum Modulator {
    Cc(u8, u8),
    InvertCc(u8, u8),
    InvertMaxCc(u8, u8, u8),
    TriggerWhen(TriggerCondition, (u8, u8)),
    MaxCc(u8, u8, u8),
    PolarCcSwitch {
        cc_low: Option<u8>,
        cc_high: Option<u8>,
        cc_switch: Option<u8>,
        default: u8,
    },
    PitchBend(f64),
    Aftertouch(u8),
    PositivePitchBend(f64),
    Multi(Vec<Modulator>),
}

#[derive(Serialize, Deserialize, Clone)]
pub enum TriggerCondition {
    Gt(u8),
    Lt(u8),
}

impl TriggerCondition {
    pub fn check(&self, value: u8) -> bool {
        match self {
            TriggerCondition::Gt(v) => &value > v,
            TriggerCondition::Lt(v) => &value < v,
        }
    }
}

impl Modulator {
    pub fn all(&self) -> Vec<Modulator> {
        match self {
            Modulator::Multi(values) => values.clone(),
            _ => vec![self.clone()],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq)]
pub enum ScaleDegree {
    Second,
    Third,
    Fourth,
    Fifth,
    Sixth,
    Seventh,
}

#[derive(Serialize, Deserialize, Clone, Copy, Eq, PartialEq)]
pub enum Quality {
    Major = 0,
    Minor = -1,
}

#[derive(Serialize, Deserialize, Clone, Copy, Eq, PartialEq)]
pub enum PerfectQuality {
    Diminished = -1,
    Perfect = 0,
    Augmented = 1,
}
