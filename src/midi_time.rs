use std::ops::{Add, Div, Mul, Rem, Sub};

pub const SUB_TICKS: u8 = 4;

#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Debug, Hash)]
pub struct MidiTime {
    ticks: i32,
    sub_ticks: u8,
}

impl MidiTime {
    pub fn new(ticks: i32, sub_ticks: u8) -> MidiTime {
        if sub_ticks >= SUB_TICKS {
            MidiTime {
                ticks,
                sub_ticks: 0,
            } + MidiTime::from_sub_ticks(sub_ticks)
        } else {
            MidiTime { ticks, sub_ticks }
        }
    }
    pub fn from_ticks(ticks: i32) -> MidiTime {
        MidiTime {
            ticks,
            sub_ticks: 0,
        }
    }

    pub fn from_sub_ticks(sub_ticks: u8) -> MidiTime {
        MidiTime {
            ticks: (sub_ticks / SUB_TICKS) as i32,
            sub_ticks: sub_ticks % SUB_TICKS,
        }
    }

    pub fn zero() -> MidiTime {
        MidiTime::from_ticks(0)
    }

    pub fn tick() -> MidiTime {
        MidiTime::from_ticks(1)
    }

    pub fn half_tick() -> MidiTime {
        MidiTime::from_sub_ticks(SUB_TICKS / 2)
    }

    pub fn from_beats(beats: i32) -> MidiTime {
        MidiTime::from_ticks(beats * 24)
    }

    pub fn from_measure(beats: i32, divider: i32) -> MidiTime {
        MidiTime::from_ticks(beats * 24 / divider)
    }

    pub fn quantize_length(length: MidiTime) -> MidiTime {
        let grid = get_quantize_grid(length.ticks);
        let result = MidiTime::from_ticks(((length.ticks as f64 / grid).round() * grid) as i32);
        result
    }

    pub fn half(&self) -> MidiTime {
        if self.ticks % 2 == 0 {
            MidiTime {
                ticks: self.ticks / 2,
                sub_ticks: self.sub_ticks / 2,
            }
        } else {
            let sub_ticks = ((self.sub_ticks / 2) as i32) + (SUB_TICKS as i32 / 2);
            let mut ticks = self.ticks;
            ticks += sub_ticks / SUB_TICKS as i32;
            MidiTime {
                ticks: ticks / 2,
                sub_ticks: sub_ticks as u8,
            }
        }
    }

    pub fn is_zero(&self) -> bool {
        self.ticks == 0 && self.sub_ticks == 0
    }

    pub fn is_whole_beat(&self) -> bool {
        self.sub_ticks == 0 && self.ticks % 24 == 0
    }
    pub fn is_whole_tick(&self) -> bool {
        self.sub_ticks == 0
    }

    pub fn beat_tick(&self) -> i32 {
        self.ticks % 24
    }

    pub fn ticks(&self) -> i32 {
        self.ticks
    }

    pub fn sub_ticks(&self) -> u8 {
        self.sub_ticks
    }

    pub fn as_float(&self) -> f64 {
        (self.ticks as f64) + self.sub_ticks_float()
    }

    pub fn sub_ticks_float(&self) -> f64 {
        (self.sub_ticks as f64) / SUB_TICKS as f64
    }

    pub fn from_float(float: f64) -> MidiTime {
        let ticks = float as i32;
        let sub_ticks = ((float - ticks as f64) * SUB_TICKS as f64) as u8;
        let result = MidiTime { ticks, sub_ticks };
        result
    }

    pub fn round(&self) -> MidiTime {
        if self.sub_ticks < (SUB_TICKS / 2) {
            MidiTime {
                ticks: self.ticks,
                sub_ticks: 0,
            }
        } else {
            MidiTime {
                ticks: self.ticks + 1,
                sub_ticks: 0,
            }
        }
    }

    pub fn floor(&self) -> MidiTime {
        MidiTime::from_ticks(self.ticks)
    }

    pub fn quantize(&self, block_align: MidiTime) -> MidiTime {
        MidiTime::from_ticks((self.ticks() / block_align.ticks()) * block_align.ticks())
    }

    pub fn swing(&self, amount: f64) -> MidiTime {
        let sixteenth = MidiTime::from_ticks(6);
        let root = MidiTime::from_ticks((self.ticks() / 12) * 12);
        let offset = *self - root;

        let (up, down) = swing_multipliers(amount);

        if offset < sixteenth {
            root + MidiTime::from_float(offset.as_float() * up)
        } else {
            let sixteenth_offset = offset - sixteenth;
            let peak = sixteenth.as_float() * up;
            root + MidiTime::from_float(peak + sixteenth_offset.as_float() * down)
        }
    }
}

fn swing_multipliers(amount: f64) -> (f64, f64) {
    if amount > 0.0 {
        (1.0 - amount, 1.0 + amount)
    } else {
        (1.0 + (amount * -1.0), 1.0 - (amount * -1.0))
    }
}

impl Sub for MidiTime {
    type Output = MidiTime;

    fn sub(self, other: MidiTime) -> MidiTime {
        let ticks = if self.sub_ticks < other.sub_ticks {
            self.ticks - other.ticks - 1
        } else {
            self.ticks - other.ticks
        };
        MidiTime {
            ticks,
            sub_ticks: modulo(
                self.sub_ticks as i32 - other.sub_ticks as i32,
                SUB_TICKS as i32,
            ) as u8,
        }
    }
}

impl Add for MidiTime {
    type Output = MidiTime;

    fn add(self, other: MidiTime) -> MidiTime {
        let ticks = if (self.sub_ticks as u32) + (other.sub_ticks as u32) >= SUB_TICKS as u32 {
            self.ticks + other.ticks + 1
        } else {
            self.ticks + other.ticks
        };
        MidiTime {
            ticks,
            sub_ticks: (self.sub_ticks + other.sub_ticks) % SUB_TICKS,
        }
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
        // ignore sub_ticks on modulus
        MidiTime {
            ticks: modulo(self.ticks, modulus.ticks),
            sub_ticks: self.sub_ticks,
        }
    }
}

fn modulo(n: i32, m: i32) -> i32 {
    ((n % m) + m) % m
}

fn get_quantize_grid(length: i32) -> f64 {
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
    fn subtract() {
        let a = MidiTime {
            ticks: 100,
            sub_ticks: 5,
        };
        let b = MidiTime {
            ticks: 90,
            sub_ticks: 4,
        };
        let c = MidiTime {
            ticks: 90,
            sub_ticks: 6,
        };
        assert_eq!(
            a - b,
            MidiTime {
                ticks: 10,
                sub_ticks: 1
            }
        );
        assert_eq!(
            a - c,
            MidiTime {
                ticks: 9,
                sub_ticks: 7
            }
        );
        assert_eq!(
            MidiTime::new(1, 0) - MidiTime::new(0, 1),
            MidiTime {
                ticks: 0,
                sub_ticks: SUB_TICKS - 1
            }
        );
    }

    #[test]
    fn add() {
        let a = MidiTime {
            ticks: 100,
            sub_ticks: 4,
        };
        let b = MidiTime {
            ticks: 50,
            sub_ticks: 3,
        };
        let c = MidiTime {
            ticks: 50,
            sub_ticks: 6,
        };
        assert_eq!(
            a + b,
            MidiTime {
                ticks: 150,
                sub_ticks: 7
            }
        );
        assert_eq!(
            a + c,
            MidiTime {
                ticks: 151,
                sub_ticks: 2
            }
        );
        assert_eq!(
            MidiTime::new(0, SUB_TICKS - 1) + MidiTime::new(0, 1),
            MidiTime {
                ticks: 1,
                sub_ticks: 0
            }
        );
    }

    #[test]
    fn sub_tick_wrap_around() {
        assert_eq!(
            MidiTime::from_sub_ticks(SUB_TICKS),
            MidiTime {
                ticks: 1,
                sub_ticks: 0
            }
        );
    }

    #[test]
    fn half() {
        // TODO: test fractions, etc
        let a = MidiTime::from_beats(4);
        assert_eq!(
            a.half(),
            MidiTime {
                ticks: 4 * 24 / 2,
                sub_ticks: 0
            }
        );
    }

    #[test]
    fn swing() {
        assert_eq!(
            MidiTime::from_ticks(24).swing(0.5),
            MidiTime::from_ticks(24)
        );
        assert_eq!(
            MidiTime::from_ticks(24 * 2).swing(0.5),
            MidiTime::from_ticks(24 * 2)
        );
        assert_eq!(
            MidiTime::from_ticks(24 * 3).swing(0.5),
            MidiTime::from_ticks(24 * 3)
        );

        // TODO: I DON'T THINK THIS IS WORKING!???
        assert_eq!(
            MidiTime::from_ticks(24 + 6).swing(0.5),
            MidiTime::from_ticks(27)
        );
        // assert_eq!(MidiTime::from_ticks(24 * 2 + 6).swing(0.5), MidiTime::from_ticks(24 * 2 + 6));
        // assert_eq!(MidiTime::from_ticks(24 * 3 + 6).swing(0.5), MidiTime::from_ticks(24 * 3 + 6));
    }

    #[test]
    fn float_conversion() {
        assert_eq!(MidiTime::new(0, 0).as_float(), 0.0);
        assert_eq!(MidiTime::new(0, SUB_TICKS / 2).as_float(), 0.5);
        assert_eq!(MidiTime::new(1, SUB_TICKS / 2).as_float(), 1.5);
        assert_eq!(MidiTime::new(2, 0).as_float(), 2.0);
        assert_eq!(MidiTime::from_float(0.0), MidiTime::new(0, 0));
        assert_eq!(MidiTime::from_float(0.5), MidiTime::new(0, SUB_TICKS / 2));
        assert_eq!(MidiTime::from_float(1.5), MidiTime::new(1, SUB_TICKS / 2));
        assert_eq!(MidiTime::from_float(2.0), MidiTime::new(2, 0));
    }
}
