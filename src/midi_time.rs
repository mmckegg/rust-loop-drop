use std::ops::{Add, Sub, Mul, Div, Rem};

#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Debug, Hash)]
pub struct MidiTime {
    ticks: i32,
    fraction: u8
}

impl MidiTime {
    pub fn new (ticks: i32, fraction: u8) -> MidiTime {
        MidiTime { ticks, fraction }
    }
    pub fn from_ticks (ticks: i32) -> MidiTime {
        MidiTime { ticks, fraction: 0 }
    }

    pub fn from_frac (fraction: u8) -> MidiTime {
        MidiTime { ticks: 0, fraction }
    }

    pub fn zero () -> MidiTime {
        MidiTime::from_ticks(0)
    }

    pub fn tick () -> MidiTime {
        MidiTime::from_ticks(1)
    }

    pub fn half_tick () -> MidiTime {
        MidiTime::from_frac(127)
    }

    pub fn from_beats (beats: i32) -> MidiTime {
        MidiTime::from_ticks(beats * 24)
    }

    pub fn from_measure (beats: i32, divider: i32) -> MidiTime {
        MidiTime::from_ticks(beats * 24 / divider)
    }

    pub fn quantize_length (length: MidiTime) -> MidiTime {
        let grid = get_quantize_grid(length.ticks);
        let result = MidiTime::from_ticks(((length.ticks as f64 / grid).round() * grid) as i32);
        result
    }

    pub fn half (&self) -> MidiTime {
        if self.ticks % 2 == 0 {
            MidiTime {ticks: self.ticks / 2, fraction: self.fraction / 2 }
        } else {
            let fraction = ((self.fraction / 2) as i32) + 127;
            let mut ticks = self.ticks;
            ticks += fraction / 256;
            MidiTime {ticks: ticks / 2, fraction: fraction as u8 }
        }
    }

    pub fn is_zero (&self) -> bool {
        self.ticks == 0 && self.fraction == 0
    }

    pub fn is_whole_beat (&self) -> bool {
        self.fraction == 0 && self.ticks % 24 == 0
    }

    pub fn beat_tick (&self) -> i32 {
        self.ticks % 24
    }

    pub fn ticks (&self) -> i32 {
        self.ticks
    }

    pub fn frac (&self) -> u8 {
        self.fraction
    }

    pub fn round (&self) -> MidiTime {
        if self.fraction < 128 {
            MidiTime {ticks: self.ticks, fraction: 0}
        } else {
            MidiTime {ticks: self.ticks + 1, fraction: 0}
        }
    }

    pub fn whole (&self) -> MidiTime {
        if self.fraction >= 128 {
            MidiTime::from_ticks(self.ticks + 1)
        } else {
            MidiTime::from_ticks(self.ticks)
        }
    }
}

impl Sub for MidiTime {
    type Output = MidiTime;

    fn sub(self, other: MidiTime) -> MidiTime {
        let ticks = if self.fraction < other.fraction {
            self.ticks - other.ticks - 1
        } else {
            self.ticks - other.ticks
        };
        MidiTime { ticks, fraction: self.fraction.wrapping_sub(other.fraction) }
    }
}

impl Add for MidiTime {
    type Output = MidiTime;

    fn add(self, other: MidiTime) -> MidiTime {
        let ticks = if (self.fraction as u32) + (other.fraction as u32) > u8::max_value() as u32 {
            self.ticks + other.ticks + 1
        } else {
            self.ticks + other.ticks
        };
        MidiTime { ticks, fraction: self.fraction.wrapping_add(other.fraction) }
    }
}

impl Mul<i32> for MidiTime {
    type Output = Self;

    fn mul(self, rhs: i32) -> Self {
        MidiTime::from_ticks(self.ticks * rhs)
    }
}

impl Div<i32> for MidiTime {
    type Output = Self;

    fn div(self, rhs: i32) -> Self {
        MidiTime::from_ticks(self.ticks / rhs)
    }
}


impl Rem<MidiTime> for MidiTime {
    type Output = MidiTime;

    fn rem(self, modulus: MidiTime) -> Self {
        // ignore fraction on modulus
        MidiTime { ticks: modulo(self.ticks, modulus.ticks), fraction: self.fraction }
    }
}

fn modulo (n: i32, m: i32) -> i32 {
    ((n % m) + m) % m
}

fn get_quantize_grid (length: i32) -> f64 {
  if length < 24 - 8 {
    24.0 / 2.0
  } else if length < 24 + 16 {
    24.0
  } else {
    24.0 * 2.0
  }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtract () {
        let a = MidiTime { ticks: 100, fraction: 100 };
        let b = MidiTime { ticks: 90, fraction: 90 };
        let c = MidiTime { ticks: 90, fraction: 110 };
        assert_eq!(a - b, MidiTime { ticks: 10, fraction: 10 });
        assert_eq!(a - c, MidiTime { ticks: 9, fraction: 246 });
    }

    #[test]
    fn add () {
        let a = MidiTime { ticks: 100, fraction: 100 };
        let b = MidiTime { ticks: 50, fraction: 90 };
        let c = MidiTime { ticks: 50, fraction: 200 };
        assert_eq!(a + b, MidiTime { ticks: 150, fraction: 190 });
        assert_eq!(a + c, MidiTime { ticks: 151, fraction: 44 });
    }

    #[test]
    fn half () {
        // TODO: test fractions, etc
        let a = MidiTime::from_beats(4);
        assert_eq!(a.half(), MidiTime { ticks: 4 * 24 / 2, fraction: 0 });
    }
}