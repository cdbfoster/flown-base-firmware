use core::f32;

use alloc::boxed::Box;
use async_trait::async_trait;
use embassy_futures::yield_now;
use embassy_time::{Duration, Instant};
use micromath::F32Ext;

use crate::effect::{DisplayMode, Effect, EffectBuffer, EffectEvent, EffectId};
use crate::render::{Rgb, LED_COUNT};

pub struct SinePulseEffect {
    id: Option<EffectId>,
    start: Instant,
    period: Duration,
    amplitude: f32,
    offset: f32,
    wrapped: Option<EffectBuffer>,
}

impl SinePulseEffect {
    pub fn new(
        id: Option<EffectId>,
        period: Duration,
        amplitude: f32,
        offset: f32,
        wrapped: Option<Box<dyn Effect>>,
    ) -> Self {
        Self {
            id,
            start: Instant::now(),
            period,
            amplitude,
            offset,
            wrapped: wrapped.map(|effect| EffectBuffer::new(effect, LED_COUNT)),
        }
    }
}

#[async_trait]
impl Effect for SinePulseEffect {
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

        if self.start.elapsed() >= self.period {
            self.start = Instant::now();
        }

        None
    }

    async fn apply(&mut self, buffer: &mut [Rgb]) {
        if let Some(EffectBuffer { effect, buffer }) = self.wrapped.as_mut() {
            effect.apply(buffer).await;
        }

        let t = self.start.elapsed().as_micros() as f32 / self.period.as_micros() as f32;
        let a = (2.0 * f32::consts::PI * t).sin() * self.amplitude + self.offset;

        for (i, pixel) in buffer.iter_mut().enumerate() {
            let wrapped = self
                .wrapped
                .as_ref()
                .map(|w| w.buffer[i])
                .unwrap_or(Rgb::BLACK);

            let new_pixel = pixel.lerp(wrapped, a);
            *pixel = new_pixel;

            if i % 2 == 0 {
                yield_now().await;
            }
        }
    }
}
