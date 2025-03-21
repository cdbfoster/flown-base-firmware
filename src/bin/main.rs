#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use log::info;

use firmware::effect::{Effect, FadeCurve, FadeDirection, FadeTransitionEffect, SinePulseEffect};
use firmware::event::{button_input, charger_input, Event};
use firmware::power::PowerState;
use firmware::render::{renderer, Rgb};
use firmware::state::{Mode, MutexGuard, State};

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    esp_alloc::heap_allocator!(size: 256 * 1024);
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let hal = esp_hal::init(config);

    let state = State::initialize(hal).await;

    spawner.spawn(button_input()).unwrap();
    spawner.spawn(charger_input()).unwrap();
    spawner.spawn(renderer()).unwrap();

    let mut initial_hold = false;

    'main: loop {
        match state.get_mode().await {
            Mode::PreStartup => {
                // Give everything a short time to set initial values.
                Timer::after_millis(1).await;
                initial_hold = state.get_button_state().await.is_held();
                state.set_mode(Mode::Startup).await;
            }
            Mode::Startup => {
                if state.get_charger_state().await.is_plugged_in() {
                    state.set_mode(Mode::PreCharging).await;
                    continue 'main;
                }

                if !initial_hold {
                    state.set_mode(Mode::Shutdown).await;
                    continue 'main;
                }

                match state.events.receive().await {
                    Event::ButtonHold => {
                        info!("Turning on.");
                        state.power.lock().await.state = PowerState::On;
                        state.set_mode(Mode::PreMain).await;
                        continue 'main;
                    }
                    Event::ButtonPress | Event::ButtonRelease => {
                        initial_hold = false;
                        state.set_mode(Mode::Shutdown).await;
                        continue 'main;
                    }
                    Event::ChargerPluggedIn => {
                        state.set_mode(Mode::PreCharging).await;
                        continue 'main;
                    }
                    _ => (),
                }
            }
            Mode::PreCharging => {
                let charging_effect: Vec<Box<dyn Effect>> = vec![
                    Box::new(Rgb::WHITE),
                    Box::new(SinePulseEffect::new(
                        None,
                        Duration::from_millis(5000),
                        0.075,
                        0.85,
                        None,
                    )),
                ];

                let mut effect_stack = state.effect_stack.lock().await;
                add_fade_in(&mut effect_stack, Some(Box::new(charging_effect)), 1000);

                state.set_mode(Mode::Charging).await;
                continue 'main;
            }
            Mode::Charging => {
                info!("Charging...");
                match state.events.receive().await {
                    Event::ButtonHold => {
                        info!("Button held!");
                        let mut power = state.power.lock().await;
                        power.state = !power.state;
                    }
                    Event::ChargerUnplugged => match state.power.lock().await.state {
                        PowerState::On => {
                            let mut effect_stack = state.effect_stack.lock().await;
                            let bundle: Vec<_> = effect_stack.drain(..).collect();
                            add_fade_out(&mut effect_stack, Some(Box::new(bundle)), 1500);

                            state.set_mode(Mode::PreMain).await;
                            continue 'main;
                        }
                        PowerState::Off => {
                            state.set_mode(Mode::Shutdown).await;
                            continue 'main;
                        }
                    },
                    _ => (),
                }
            }
            Mode::PreMain => {
                let main_effect: Vec<Box<dyn Effect>> = vec![
                    // Solid cyan.
                    Box::new(Rgb::new(0.0, 1.0, 1.0)),
                    // Pulse with red.
                    Box::new(SinePulseEffect::new(
                        None,
                        Duration::from_millis(3000),
                        0.5,
                        0.5,
                        Some(Box::new(Rgb::new(1.0, 0.0, 0.0))),
                    )),
                ];

                let mut effect_stack = state.effect_stack.lock().await;
                add_fade_in(&mut effect_stack, Some(Box::new(main_effect)), 1000);

                state.set_mode(Mode::Main).await;
                continue 'main;
            }
            Mode::Main => match state.events.receive().await {
                Event::ButtonPress => {
                    info!("Button press!");
                    initial_hold = false;
                }
                Event::ButtonHold => {
                    info!("Button hold!");
                    if initial_hold {
                        state.set_mode(Mode::Pairing).await;
                        continue 'main;
                    } else {
                        state.power.lock().await.state = PowerState::Off;
                        state.set_mode(Mode::Shutdown).await;
                        continue 'main;
                    }
                }
                Event::ButtonRelease => {
                    info!("Button release!");
                    initial_hold = false;
                }
                Event::ChargerPluggedIn => {
                    let mut effect_stack = state.effect_stack.lock().await;
                    let bundle: Vec<_> = effect_stack.drain(..).collect();
                    add_fade_out(&mut effect_stack, Some(Box::new(bundle)), 1500);

                    state.set_mode(Mode::PreCharging).await;
                }
                _ => (),
            },
            Mode::PrePairing => {
                state.set_mode(Mode::Pairing).await;
                continue 'main;
            }
            Mode::Pairing => {
                info!("Pairing...");
                Timer::after_millis(3000).await;
            }
            Mode::Shutdown => {
                let effect_fade_out = {
                    let mut effect_stack = state.effect_stack.lock().await;
                    if !effect_stack.is_empty() {
                        let bundle: Vec<_> = effect_stack.drain(..).collect();
                        add_fade_out(&mut effect_stack, Some(Box::new(bundle)), 500);
                        Timer::after_millis(500)
                    } else {
                        Timer::after_millis(0)
                    }
                };

                // Debounce the button if necessary.
                if state.get_button_state().await.is_held() {
                    info!("Debouncing the button...");
                    while !matches!(state.events.receive().await, Event::ButtonRelease) {}
                }

                effect_fade_out.await;

                info!("Shutting down.");
                state.exit.signal(());

                // Give the handlers a bit to shut down and release their pins.
                Timer::after_millis(1).await;

                // Safe because all handlers should be shut down now, so all GPIOs should be free.
                unsafe {
                    info!("Turning off.");
                    state.power.lock().await.turn_off();
                }
            }
        }
    }
}

fn add_fade_in(
    effect_stack: &mut MutexGuard<'_, Vec<Box<dyn Effect>>>,
    effect: Option<Box<dyn Effect>>,
    duration: u64,
) {
    effect_stack.push(Box::new(FadeTransitionEffect::new(
        None,
        Duration::from_millis(duration),
        FadeCurve::Linear,
        FadeDirection::In,
        effect,
    )));
}

fn add_fade_out(
    effect_stack: &mut MutexGuard<'_, Vec<Box<dyn Effect>>>,
    effect: Option<Box<dyn Effect>>,
    duration: u64,
) {
    effect_stack.push(Box::new(FadeTransitionEffect::new(
        None,
        Duration::from_millis(duration),
        FadeCurve::Linear,
        FadeDirection::Out,
        effect,
    )));
}
