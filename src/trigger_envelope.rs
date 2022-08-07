pub struct TriggerEnvelope {
    pub tick_multiplier: f64,
    pub max_tick_change: f64,
    pub tick_value: f64,
    value: f64,
    out_value: f64,
}

impl TriggerEnvelope {
    pub fn new(tick_multiplier: f64, max_tick_change: f64) -> Self {
        TriggerEnvelope {
            tick_multiplier,
            max_tick_change,
            value: 0.0,
            out_value: 0.0,
            tick_value: 1.0,
        }
    }

    pub fn value(&self) -> f64 {
        self.out_value.max(0.0).min(1.0)
    }

    pub fn tick(&mut self, triggered: bool) {
        // decay
        self.value = if triggered && self.tick_multiplier > 0.0 {
            self.tick_value
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
