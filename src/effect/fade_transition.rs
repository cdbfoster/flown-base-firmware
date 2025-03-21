use core::f32;

use alloc::boxed::Box;
use async_trait::async_trait;
use embassy_futures::yield_now;
use embassy_time::{Duration, Instant};

use crate::effect::{DisplayMode, Effect, EffectBuffer, EffectEvent, EffectId};
use crate::render::{Rgb, LED_COUNT};

pub enum FadeDirection {
    In,
    Out,
}

impl FadeDirection {
    fn apply(&self, x: f32) -> f32 {
        match self {
            Self::In => x,
            Self::Out => 1.0 - x,
        }
    }
}

pub enum FadeCurve {
    Linear,
    EaseIn,
    EaseOut,
    Smoothstep,
}

impl FadeCurve {
    fn apply(&self, x: f32) -> f32 {
        match self {
            Self::Linear => x,
            Self::EaseIn => x * x * x,
            Self::EaseOut => 1.0 - (1.0 - x) * (1.0 - x) * (1.0 - x),
            Self::Smoothstep => x * x * (3.0 - 2.0 * x),
        }
    }
}

pub struct FadeTransitionEffect {
    id: Option<EffectId>,
    start: Instant,
    duration: Duration,
    fade_curve: FadeCurve,
    fade_direction: FadeDirection,
    wrapped: Option<EffectBuffer>,
}

impl FadeTransitionEffect {
    pub fn new(
        id: Option<EffectId>,
        period: Duration,
        fade_curve: FadeCurve,
        fade_direction: FadeDirection,
        wrapped: Option<Box<dyn Effect>>,
    ) -> Self {
        Self {
            id,
            start: Instant::now(),
            duration: period,
            fade_curve,
            fade_direction,
            wrapped: wrapped.map(|effect| EffectBuffer::new(effect, LED_COUNT)),
        }
    }
}

#[async_trait]
impl Effect for FadeTransitionEffect {
    fn id(&self) -> Option<EffectId> {
        self.id
    }

    fn display_mode(&self) -> DisplayMode {
        DisplayMode::Blend
    }

    fn update(&mut self, elapsed: Duration) -> Option<EffectEvent> {
        if let Some(EffectBuffer { effect, .. }) = self.wrapped.as_mut() {
            match effect.update(elapsed) {
                Some(EffectEvent::Replace(new_effect)) => {
                    *effect = new_effect;
                }
                Some(EffectEvent::Remove) => {
                    self.wrapped = None;
                    return Some(EffectEvent::Remove);
                }
                None => (),
            }
        }

        if self.start.elapsed() >= self.duration {
            return Some(match self.wrapped.take() {
                Some(EffectBuffer { effect, .. }) => match self.fade_direction {
                    FadeDirection::In => EffectEvent::Replace(effect),
                    FadeDirection::Out => EffectEvent::Remove,
                },
                None => EffectEvent::Remove,
            });
        }

        None
    }

    async fn apply(&mut self, buffer: &mut [Rgb]) {
        if let Some(EffectBuffer { effect, buffer }) = self.wrapped.as_mut() {
            effect.apply(buffer).await;
        }

        let mut t = self.start.elapsed().as_micros() as f32 / self.duration.as_micros() as f32;
        t = self.fade_curve.apply(t);
        t = self.fade_direction.apply(t);

        for (i, pixel) in buffer.iter_mut().enumerate() {
            let wrapped = self
                .wrapped
                .as_ref()
                .map(|w| w.buffer[i])
                .unwrap_or(Rgb::BLACK);

            let new_pixel = pixel.lerp(wrapped, t);
            *pixel = new_pixel;

            if i % 2 == 0 {
                yield_now().await;
            }
        }
    }
}
