use embassy_futures::yield_now;
use embassy_time::Instant;
use esp_hal::gpio::Level;
use esp_hal::rmt::{Rmt, TxChannelConfig, TxChannelCreatorAsync};
use esp_hal::time::Rate;
use log::info;

use crate::effect::Effect;
use crate::state::State;

use self::async_transmit::transmit;
pub use self::rgb::Rgb;

mod async_transmit;
mod rgb;

pub const LED_COUNT: usize = 200;

// Hardcoded because esp_hal::rmt::PulseCode::new is not const.
// These values are only valid for an RMT frequency of 80MHz.
pub(crate) const ONE: u32 = 2392128; // PulseCode::new(Level::High, 64, Level::Low, 36)
pub(crate) const ZERO: u32 = 4227108; // PulseCode::new(Level::High, 36, Level::Low, 64)

#[embassy_executor::task]
pub async fn renderer() {
    let state = State::get().await;

    // Effects write to the render buffer.
    let mut render_buffer = [Rgb::BLACK; LED_COUNT];
    // The render buffer is translated into pulse codes, which are sent to the remote control module.
    let mut pulse_buffer = [ZERO; LED_COUNT * 24 + 1];
    *pulse_buffer.last_mut().unwrap() = 0;

    let mut rmt_channel = {
        let mut peripherals = state.peripherals.lock().await;

        let rmt_peripheral = peripherals.rmt.take().expect("rmt already taken");
        let freq = Rate::from_mhz(80);
        let rmt = Rmt::new(rmt_peripheral, freq)
            .expect("could not initialize rmt")
            .into_async();

        let tx_config = TxChannelConfig::default()
            .with_clk_divider(1)
            .with_idle_output(true)
            .with_idle_output_level(Level::Low)
            .with_carrier_modulation(false);

        let signal_pin = peripherals
            .signal_1_pin
            .take()
            .expect("signal 1 pin already taken");

        rmt.channel0
            .configure(signal_pin, tx_config)
            .expect("could not initialize signal 1")
    };

    let mut fps_acc = 0;
    let mut fps_time = Instant::now();
    let mut effect_time = Instant::now();
    loop {
        let frame_start = Instant::now();

        // Clear buffer.
        render_buffer.fill(Rgb::BLACK);

        // Update and render effects.
        {
            let mut effect_stack = state.effect_stack.lock().await;
            effect_stack.update(effect_time.elapsed());
            effect_stack.apply(&mut render_buffer).await;
        }
        effect_time = Instant::now();
        let t_a = frame_start.elapsed().as_micros();

        // Translate the render buffer into pulses.
        write_pulses(&render_buffer, &mut pulse_buffer, Rgb::WHITE).await;
        let t_b = frame_start.elapsed().as_micros() - t_a;

        // Transmit the pulses on the RMT.
        transmit(&mut rmt_channel, &pulse_buffer)
            .await
            .expect("could not transmit pulses");
        let t_c = frame_start.elapsed().as_micros() - t_b - t_a;

        fps_acc += 1;
        if fps_time.elapsed().as_millis() >= 1000 {
            fps_time = Instant::now();
            info!(
                "FPS: {fps_acc}, effects({}): {t_a}, pulses: {t_b}, transmit: {t_c}",
                state.effect_stack.lock().await.len(),
            );
            fps_acc = 0;
        }
    }
}

async fn write_pulses(render_buffer: &[Rgb], pulse_buffer: &mut [u32], color_correction: Rgb) {
    let data = render_buffer
        .iter()
        // Gamma correction
        .map(|pixel| Rgb {
            r: pixel.r * pixel.r,
            g: pixel.g * pixel.g,
            b: pixel.b * pixel.b,
        })
        // Color correction
        .map(|pixel| Rgb {
            r: pixel.r * color_correction.r,
            g: pixel.g * color_correction.g,
            b: pixel.b * color_correction.b,
        })
        .enumerate();

    for (i, pixel) in data {
        pixel.write_pulses(&mut pulse_buffer[i * 24..(i + 1) * 24]);

        // Very non-scientific measurements suggest that this is about
        // once every 25 microseconds.
        if i % 2 == 0 {
            yield_now().await;
        }
    }
}
