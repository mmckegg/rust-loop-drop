use ::output_value::OutputValue;
use ::midi_time::MidiTime;

#[derive(PartialEq, Debug, Clone)]
pub enum LoopTransform {
    Value(OutputValue),
    Repeat { rate: MidiTime, offset: MidiTime, value: OutputValue },
    Range { pos: MidiTime, length: MidiTime },
    None
}

impl LoopTransform {
    pub fn apply (&self, previous: &LoopTransform) -> LoopTransform {
        match self {
            &LoopTransform::Range {pos, length} => {
                match previous {
                    &LoopTransform::Repeat {rate, offset, value} => {
                        LoopTransform::Repeat {
                            rate: rate.max(length), offset, value
                        }
                    },
                    &LoopTransform::Range {pos: previous_pos, length: previous_length} => {
                        let playback_offset = pos % previous_length;
                        let playback_pos = previous_pos + ((pos - playback_offset) % previous_length);
                        LoopTransform::Range {
                            pos: playback_pos,
                            length
                        }
                    },
                    _ => self.clone()
                }
            },
            &LoopTransform::None => previous.clone(),
            _ => self.clone()
        }
    }

    pub fn is_active (&self) -> bool {
        match self {
            &LoopTransform::Value(OutputValue::Off) | &LoopTransform::None => false,
            _ => true
        }
    }
}