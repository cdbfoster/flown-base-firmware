use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel as EmbassyChannel;
use embassy_sync::mutex::{Mutex as EmbassyMutex, MutexGuard as EmbassyMutexGuard};
use embassy_sync::once_lock::OnceLock;
use embassy_sync::signal::Signal as EmbassySignal;
use esp_hal::gpio::{GpioPin, Level, Output, OutputConfig};
use esp_hal::peripherals::{Peripherals as HalPeripherals, RMT};
use esp_hal::timer::timg::TimerGroup;

use crate::effect::Effect;
use crate::event::{ButtonState, ChargerState, Event};
use crate::power::Power;

static STATE: OnceLock<State> = OnceLock::new();

pub struct State {
    pub mode: Mutex<Mode>,
    pub peripherals: Mutex<Peripherals>,
    pub button_state: Mutex<ButtonState>,
    pub charger_state: Mutex<ChargerState>,
    pub events: Channel<Event, 10>,
    pub exit: Signal<()>,
    pub power: Mutex<Power>,
    pub effect_stack: Mutex<Vec<Box<dyn Effect>>>,
}

pub type Channel<T, const N: usize> = EmbassyChannel<NoopRawMutex, T, N>;
pub type Mutex<T> = EmbassyMutex<NoopRawMutex, T>;
pub type MutexGuard<'a, T> = EmbassyMutexGuard<'a, NoopRawMutex, T>;
pub type Signal<T> = EmbassySignal<NoopRawMutex, T>;

impl State {
    pub async fn initialize(hal: HalPeripherals) -> &'static Self {
        // Temporary while debugging, to indicate that the chip is on.
        let _on_light = Output::new(hal.GPIO8, Level::High, OutputConfig::default());

        let timg0 = TimerGroup::new(hal.TIMG0);
        let timg1 = TimerGroup::new(hal.TIMG1);
        esp_hal_embassy::init([timg0.timer0, timg1.timer0]);

        STATE
            .init(State {
                mode: Mutex::new(Mode::PreStartup),
                peripherals: Mutex::new(Peripherals {
                    battery_monitor_pin: Some(hal.GPIO3),
                    charger_pin: Some(hal.GPIO4),
                    button_pin: Some(hal.GPIO5),
                    signal_1_pin: Some(hal.GPIO6),
                    signal_2_pin: Some(hal.GPIO7),
                    rmt: Some(hal.RMT),
                }),
                button_state: Mutex::new(ButtonState::NotHeld),
                charger_state: Mutex::new(ChargerState::Unplugged),
                events: Channel::new(),
                exit: Signal::new(),
                power: Mutex::new(Power::new(hal.LPWR)),
                effect_stack: Mutex::new(Vec::new()),
            })
            .expect("can't be set already");

        Self::get().await
    }

    pub async fn get() -> &'static Self {
        STATE.get().await
    }

    pub async fn get_mode(&self) -> Mode {
        *self.mode.lock().await
    }

    pub async fn set_mode(&self, mode: Mode) {
        *self.mode.lock().await = mode;
    }

    pub async fn get_button_state(&self) -> ButtonState {
        *self.button_state.lock().await
    }

    pub async fn get_charger_state(&self) -> ChargerState {
        *self.charger_state.lock().await
    }
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("State").finish()
    }
}

#[derive(Clone, Copy)]
pub enum Mode {
    PreStartup,
    Startup,
    PreCharging,
    Charging,
    PreMain,
    Main,
    PrePairing,
    Pairing,
    Shutdown,
}

pub(crate) type ChargerPin = GpioPin<4>;
pub(crate) type ButtonPin = GpioPin<5>;

pub struct Peripherals {
    pub battery_monitor_pin: Option<GpioPin<3>>,
    pub charger_pin: Option<ChargerPin>,
    pub button_pin: Option<ButtonPin>,
    pub signal_1_pin: Option<GpioPin<6>>,
    pub signal_2_pin: Option<GpioPin<7>>,
    pub rmt: Option<RMT>,
}
