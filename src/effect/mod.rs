use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use async_trait::async_trait;
use embassy_time::Duration;

use crate::render::Rgb;

/*
pub use self::sine_pulse::SinePulseEffect;
pub use self::solid::SolidEffect;

mod sine_pulse;
mod solid; */

pub use self::fade_transition::{FadeCurve, FadeDirection, FadeTransitionEffect};
pub use self::sine_pulse::SinePulseEffect;

mod fade_transition;
mod sine_pulse;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DisplayMode {
    Blend,
    Opaque,
}

pub enum EffectEvent {
    Replace(Box<dyn Effect>),
    Remove,
}

#[async_trait]
pub trait Effect: Send + Sync {
    fn id(&self) -> Option<EffectId>;
    fn display_mode(&self) -> DisplayMode;
    fn update(&mut self, elapsed: Duration) -> Option<EffectEvent>;
    async fn apply(&mut self, buffer: &mut [Rgb]);
}

mod core_implementations {
    use super::*;

    #[async_trait]
    impl Effect for Box<dyn Effect> {
        fn id(&self) -> Option<EffectId> {
            self.as_ref().id()
        }

        fn display_mode(&self) -> DisplayMode {
            self.as_ref().display_mode()
        }

        fn update(&mut self, elapsed: Duration) -> Option<EffectEvent> {
            self.as_mut().update(elapsed)
        }

        async fn apply(&mut self, buffer: &mut [Rgb]) {
            self.as_mut().apply(buffer).await
        }
    }

    #[async_trait]
    impl Effect for Vec<Box<dyn Effect>> {
        fn id(&self) -> Option<EffectId> {
            None
        }

        fn display_mode(&self) -> DisplayMode {
            self.iter()
                .map(Effect::display_mode)
                .find(|&mode| mode == DisplayMode::Opaque)
                .unwrap_or(DisplayMode::Blend)
        }

        fn update(&mut self, elapsed: Duration) -> Option<EffectEvent> {
            let mut removals = Vec::with_capacity(self.len());

            for (i, effect) in self.iter_mut().enumerate() {
                match effect.update(elapsed) {
                    Some(EffectEvent::Replace(new_effect)) => *effect = new_effect,
                    Some(EffectEvent::Remove) => removals.push(i),
                    None => (),
                }
            }

            for i in removals.into_iter().rev() {
                self.remove(i);
            }

            None
        }

        async fn apply(&mut self, buffer: &mut [Rgb]) {
            if self.is_empty() {
                return;
            }

            let last = self.len() - 1;

            // Only need to compute from the latest opaque effect.
            let first = last
                - self
                    .iter()
                    .rev()
                    .position(|effect| effect.display_mode() == DisplayMode::Opaque)
                    .unwrap_or(last);

            for effect in self.iter_mut().take(last + 1).skip(first) {
                effect.apply(buffer).await;
            }
        }
    }

    #[async_trait]
    impl Effect for Rgb {
        fn id(&self) -> Option<EffectId> {
            None
        }

        fn display_mode(&self) -> DisplayMode {
            DisplayMode::Opaque
        }

        fn update(&mut self, _elapsed: Duration) -> Option<EffectEvent> {
            None
        }

        async fn apply(&mut self, buffer: &mut [Rgb]) {
            buffer.fill(*self);
        }
    }
}

struct EffectBuffer {
    effect: Box<dyn Effect>,
    buffer: Vec<Rgb>,
}

impl EffectBuffer {
    fn new(effect: Box<dyn Effect>, size: usize) -> Self {
        Self {
            effect,
            buffer: vec![Rgb::BLACK; size],
        }
    }
}
