// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Spring animation of a scalar value.

/// Threshold below which the spring counts as stopped.
const REST: f32 = 0.05;

/// Spring stiffness and damping.
///
/// Damping ratio ζ = c / (2√k): below 1 overshoots the target and comes back,
/// around 1 is the fastest approach without overshoot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    pub k: f32,
    pub c: f32,
}

impl Params {
    pub const DEFAULT: Self = Self { k: 320.0, c: 30.0 };
}

/// A value pulled toward its target by a spring.
#[derive(Debug, Clone, Copy)]
pub struct Spring {
    pub x: f32,
    pub v: f32,
    pub target: f32,
    pub params: Params,
}

impl Spring {
    pub fn new(x: f32) -> Self {
        Self { x, v: 0.0, target: x, params: Params::DEFAULT }
    }

    /// Moves the value to `x` instantly, without animation.
    pub fn snap(&mut self, x: f32) {
        self.x = x;
        self.v = 0.0;
        self.target = x;
    }

    /// Sets a new target and the parameters to reach it. The current velocity is kept, so a
    /// direction change mid-motion stays smooth.
    pub fn go(&mut self, target: f32, params: Params) {
        self.target = target;
        self.params = params;
    }

    /// Advances the simulation by `dt` seconds. Returns `true` while the spring is moving.
    pub fn step(&mut self, dt: f32) -> bool {
        // A large step (e.g. after idling) makes explicit Euler blow up, so clamp it.
        let dt = dt.min(0.032);
        let Params { k, c } = self.params;
        let a = -k * (self.x - self.target) - c * self.v;
        self.v += a * dt;
        self.x += self.v * dt;
        if self.v.abs() < REST && (self.x - self.target).abs() < REST {
            self.x = self.target;
            self.v = 0.0;
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(mut s: Spring) -> (Spring, f32) {
        let mut peak = s.x;
        let mut frames = 0;
        while s.step(1.0 / 60.0) {
            peak = peak.max(s.x);
            frames += 1;
            assert!(frames < 600, "the spring did not settle");
        }
        (s, peak)
    }

    #[test]
    fn settles_on_target() {
        let mut s = Spring::new(0.0);
        s.go(100.0, Params::DEFAULT);
        assert_eq!(run(s).0.x, 100.0);
    }

    #[test]
    fn underdamped_overshoots() {
        let mut s = Spring::new(0.0);
        s.go(100.0, Params { k: 230.0, c: 19.0 });
        assert!(run(s).1 > 100.5);
    }

    #[test]
    fn critically_damped_does_not_overshoot() {
        let mut s = Spring::new(0.0);
        s.go(100.0, Params { k: 420.0, c: 42.0 });
        assert!(run(s).1 <= 100.0 + REST);
    }
}
