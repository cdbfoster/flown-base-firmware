use embassy_futures::yield_now;
use esp_hal::constants;
use esp_hal::rmt::{Error, Event, RmtTxFuture, TxChannelAsync};

/// This is a workaround for esp-hal's crappy [`TxChannelAsync::transmit`] method.
pub async fn transmit<C>(channel: &mut C, data: &[u32]) -> Result<(), Error>
where
    C: TxChannelAsync,
{
    C::clear_interrupts();
    C::listen_interrupt(Event::End | Event::Error);

    // Load the first chunk into RMT memory and begin the transmission.
    let mut index = C::send_raw(data, false, 0)?;

    loop {
        if C::is_error() {
            return Err(Error::TransmissionError);
        }

        if index >= data.len() {
            break;
        }

        // Each pulse code should take 1.25 microseconds to send, so
        // yielding here should be about once every 30 microseconds.
        yield_now().await;

        // Wait for the RMT to hit the threshold (half the memory sent).
        while !C::is_threshold_set() {}
        C::reset_threshold_set();

        // Refill the half of the RMT memory that's already been sent.
        let ram_index = (((index - constants::RMT_CHANNEL_RAM_SIZE)
            / (constants::RMT_CHANNEL_RAM_SIZE / 2))
            % 2)
            * (constants::RMT_CHANNEL_RAM_SIZE / 2);

        let ptr = (constants::RMT_RAM_START
            + C::CHANNEL as usize * constants::RMT_CHANNEL_RAM_SIZE * 4
            + ram_index * 4) as *mut u32;
        for (idx, entry) in data[index..]
            .iter()
            .take(constants::RMT_CHANNEL_RAM_SIZE / 2)
            .enumerate()
        {
            unsafe {
                ptr.add(idx).write_volatile(*entry);
            }
        }

        index += constants::RMT_CHANNEL_RAM_SIZE / 2;
    }

    RmtTxFuture::new(channel).await;

    if C::is_error() {
        Err(Error::TransmissionError)
    } else {
        Ok(())
    }
}
