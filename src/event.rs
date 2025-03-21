use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use embassy_futures::select::{select, select3, Either, Either3};
use embassy_time::{Duration, Instant, Timer};
use esp_hal::gpio::{Event as GpioEvent, Input, InputConfig, Pull};
use log::info;

use crate::state::State;

pub enum Event {
    ButtonPress,
    ButtonHold,
    ButtonRelease,
    ChargerPluggedIn,
    ChargerUnplugged,
}

#[derive(Clone, Copy)]
pub enum ButtonState {
    Held(Instant),
    NotHeld,
}

impl ButtonState {
    pub fn is_held(&self) -> bool {
        match self {
            Self::Held(_) => true,
            Self::NotHeld => false,
        }
    }
}

const BUTTON_HOLD_TIME: Duration = Duration::from_millis(1500);
const BUTTON_DEBOUNCE_TIME: Duration = Duration::from_millis(1);

#[embassy_executor::task]
pub async fn button_input() {
    let state = State::get().await;

    let mut input = {
        let mut peripherals = state.peripherals.lock().await;
        let pin = peripherals
            .button_pin
            .take()
            .expect("button pin already taken");
        let config = InputConfig::default().with_pull(Pull::Up);
        Input::new(pin, config)
    };

    let mut button_state = match input.is_high() {
        true => ButtonState::NotHeld,
        false => ButtonState::Held(Instant::now()),
    };
    *state.button_state.lock().await = button_state;

    let mut last_button_event = Instant::now();

    loop {
        let button = button_state.wait(&mut input);
        let hold_timer = button_state.hold_timer();
        let exit = state.exit.wait();

        match select3(button, hold_timer, exit).await {
            // Button event
            Either3::First(next_state) => {
                if last_button_event.elapsed() < BUTTON_DEBOUNCE_TIME {
                    continue;
                } else {
                    last_button_event = Instant::now();
                }

                let mut guard = state.button_state.lock().await;
                button_state = next_state;
                match next_state {
                    ButtonState::Held(_) => {
                        state.events.send(Event::ButtonPress).await;
                    }
                    ButtonState::NotHeld => {
                        state.events.send(Event::ButtonRelease).await;
                    }
                }
                *guard = next_state;
            }
            // Hold timer event
            Either3::Second(_) => {
                button_state = ButtonState::Held(Instant::now());
                state.events.send(Event::ButtonHold).await;
            }
            // Exit event
            Either3::Third(_) => {
                // Propagate the exit signal.
                state.exit.signal(());
                info!("Exiting button handler.");
                break;
            }
        }
    }
}

impl ButtonState {
    fn wait<'a, 'b>(
        &'a self,
        input: &'a mut Input<'b>,
    ) -> impl Future<Output = Self> + use<'a, 'b> {
        struct ButtonFuture<F: Future> {
            state: ButtonState,
            future: F,
        }

        impl<F: Future> Future for ButtonFuture<F> {
            type Output = ButtonState;

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let this = unsafe { self.get_unchecked_mut() };
                let future = unsafe { Pin::new_unchecked(&mut this.future) };
                if future.poll(cx).is_ready() {
                    return Poll::Ready(match this.state {
                        ButtonState::Held(_) => ButtonState::NotHeld,
                        ButtonState::NotHeld => ButtonState::Held(Instant::now()),
                    });
                }
                Poll::Pending
            }
        }

        ButtonFuture {
            state: *self,
            future: input.wait_for(match self {
                Self::Held(_) => GpioEvent::HighLevel,
                Self::NotHeld => GpioEvent::LowLevel,
            }),
        }
    }

    fn hold_timer(&self) -> impl Future {
        enum HoldTimer {
            Held(Timer),
            NotHeld,
        }

        impl Future for HoldTimer {
            type Output = ();

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let this = unsafe { self.get_unchecked_mut() };
                match this {
                    Self::Held(timer) => unsafe { Pin::new_unchecked(timer).poll(cx) },
                    Self::NotHeld => Poll::Pending,
                }
            }
        }

        match self {
            Self::Held(start_time) => HoldTimer::Held(Timer::at(*start_time + BUTTON_HOLD_TIME)),
            Self::NotHeld => HoldTimer::NotHeld,
        }
    }
}

#[derive(Clone, Copy)]
pub enum ChargerState {
    PluggedIn,
    Unplugged,
}

impl ChargerState {
    pub fn is_plugged_in(&self) -> bool {
        match self {
            Self::PluggedIn => true,
            Self::Unplugged => false,
        }
    }
}

const CHARGER_DEBOUNCE_TIME: Duration = Duration::from_millis(10);

#[embassy_executor::task]
pub async fn charger_input() {
    let state = State::get().await;

    let mut input = {
        let mut peripherals = state.peripherals.lock().await;
        let pin = peripherals
            .charger_pin
            .take()
            .expect("charger pin already taken");
        let config = InputConfig::default().with_pull(Pull::Down);
        Input::new(pin, config)
    };

    let mut charger_state = match input.is_high() {
        true => ChargerState::PluggedIn,
        false => ChargerState::Unplugged,
    };
    *state.charger_state.lock().await = charger_state;

    let mut last_charger_event = Instant::now();

    loop {
        let charger = charger_state.wait(&mut input);
        let exit = state.exit.wait();

        match select(charger, exit).await {
            // Charger event
            Either::First(next_state) => {
                if last_charger_event.elapsed() < CHARGER_DEBOUNCE_TIME {
                    continue;
                } else {
                    last_charger_event = Instant::now();
                }

                let mut guard = state.charger_state.lock().await;
                charger_state = next_state;
                state
                    .events
                    .send(match next_state {
                        ChargerState::PluggedIn => Event::ChargerPluggedIn,
                        ChargerState::Unplugged => Event::ChargerUnplugged,
                    })
                    .await;
                *guard = next_state;
            }
            // Exit event
            Either::Second(_) => {
                // Propagate the exit signal.
                state.exit.signal(());
                info!("Exiting charger handler.");
                break;
            }
        }
    }
}

impl ChargerState {
    fn wait<'a, 'b>(
        &'a self,
        input: &'a mut Input<'b>,
    ) -> impl Future<Output = Self> + use<'a, 'b> {
        struct ChargerFuture<F: Future> {
            state: ChargerState,
            future: F,
        }

        impl<F: Future> Future for ChargerFuture<F> {
            type Output = ChargerState;

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let this = unsafe { self.get_unchecked_mut() };
                let future = unsafe { Pin::new_unchecked(&mut this.future) };
                if future.poll(cx).is_ready() {
                    return Poll::Ready(match this.state {
                        ChargerState::PluggedIn => ChargerState::Unplugged,
                        ChargerState::Unplugged => ChargerState::PluggedIn,
                    });
                }
                Poll::Pending
            }
        }

        ChargerFuture {
            state: *self,
            future: input.wait_for(match self {
                Self::PluggedIn => GpioEvent::LowLevel,
                Self::Unplugged => GpioEvent::HighLevel,
            }),
        }
    }
}
