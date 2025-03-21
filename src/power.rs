use core::ops::Not;

use esp_hal::gpio::RtcPinWithResistors;
use esp_hal::peripherals::LPWR;
use esp_hal::rtc_cntl::sleep::{RtcioWakeupSource, WakeupLevel};
use esp_hal::rtc_cntl::Rtc;

use crate::state::{ButtonPin, ChargerPin};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PowerState {
    On,
    Off,
}

impl Not for PowerState {
    type Output = Self;

    fn not(self) -> Self::Output {
        match self {
            Self::On => Self::Off,
            Self::Off => Self::On,
        }
    }
}

pub struct Power {
    pub state: PowerState,
    lpwr: Option<LPWR>,
}

impl Power {
    pub fn new(lpwr: LPWR) -> Self {
        Self {
            state: PowerState::Off,
            lpwr: Some(lpwr),
        }
    }

    /// # Safety
    ///
    /// The button and the charger GPIOs must be unused at this point.
    pub unsafe fn turn_off(&mut self) -> ! {
        let mut button_pin = ButtonPin::steal();
        let mut charger_pin = ChargerPin::steal();

        let mut wakeup_pins: [(&mut dyn RtcPinWithResistors, WakeupLevel); 2] = [
            (&mut button_pin, WakeupLevel::Low),
            (&mut charger_pin, WakeupLevel::High),
        ];

        let wakeup_source = RtcioWakeupSource::new(&mut wakeup_pins);

        let lpwr = self.lpwr.take().unwrap();
        let mut rtc = Rtc::new(lpwr);
        rtc.sleep_deep(&[&wakeup_source]);
    }
}
