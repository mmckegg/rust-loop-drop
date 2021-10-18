pub struct TriggerEnvelope {
    pub tick_multiplier: f32,
    pub max_tick_change: f32,
    value: f32,
    out_value: f32,
}

impl TriggerEnvelope {
    pub fn new(tick_multiplier: f32, max_tick_change: f32) -> Self {
        TriggerEnvelope {
            tick_multiplier,
            max_tick_change,
            value: 0.0,
            out_value: 0.0,
        }
    }

    pub fn value(&self) -> f32 {
        self.out_value.max(0.0).min(1.0)
    }

    pub fn tick(&mut self, triggered: bool) {
        // decay
        self.value = if triggered {
            1.0
        } else if self.value > 0.0 {
            self.value * self.tick_multiplier
        } else {
            0.0
        };

        // slew limit
        if self.value > self.out_value {
            self.out_value += (self.value - self.out_value).min(self.max_tick_change);
        } else if self.value < self.out_value {
            self.out_value -= (self.out_value - self.value).min(self.max_tick_change);
        }
    }
}
