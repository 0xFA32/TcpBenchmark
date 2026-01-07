use std::{
    sync::{Arc, atomic::AtomicBool},
    thread,
    time::Duration,
};

use bytes::{Bytes, BytesMut};
use crossbeam_channel::Sender;

use rand::prelude::*;

const FOUR_MB: usize = 4 * 1024 * 1024;
const THREE_MB: usize = 3 * 1024 * 1024;
const TWO_MB: usize = 2 * 1024 * 1024;
static FOUR_MB_GARBAGE: [u8; FOUR_MB] = [0; FOUR_MB];
static THREE_MB_GARBAGE: [u8; THREE_MB] = [0; THREE_MB];
static TWO_MB_GARBAGE: [u8; TWO_MB] = [0; TWO_MB];

pub fn generate_data(sender: Sender<Bytes>, close: Arc<AtomicBool>) {
    let mut rng = rand::rng();
    loop {
        if close.load(std::sync::atomic::Ordering::Relaxed) {
            tracing::info!("Going to stop generating data.");
            break;
        }

        // 7 buckets of data
        // 1. [0-4KB] => 80%
        // 2. [4KB-100KB] => 5%
        // 3. [100KB-500KB] => 5%
        // 4. [500KB-1MB] => 3%
        // 5. 2MB => 3%
        // 6. 3MB => 2%
        // 7. 4MB => 2%
        let prob = rng.random_range(1..=100usize);
        let payload = if prob > 98 {
            Bytes::from_static(&FOUR_MB_GARBAGE)
        } else if prob > 96 {
            Bytes::from_static(&THREE_MB_GARBAGE)
        } else if prob > 93 {
            Bytes::from_static(&TWO_MB_GARBAGE)
        } else {
            let capacity = generate_size(&mut rng, prob);
            let mut buf = BytesMut::with_capacity(capacity);
            buf.resize(capacity, 0);
            buf.freeze()
        };

        match sender.send(payload) {
            Ok(_) => {}
            Err(e) => {
                tracing::error!("Failed to generate and send data to the channel! {e}");
            }
        }

        thread::sleep(Duration::from_millis(100));
    }
}

fn generate_size(rng: &mut ThreadRng, prob: usize) -> usize {
    match prob {
        1..=80 => rng.random_range(1..=4 * 1024),
        81..=85 => rng.random_range(4 * 1024..=100 * 1024),
        86..=90 => rng.random_range(100 * 1024..=500 * 1024),
        _ => rng.random_range(500 * 1024..=1024 * 1024),
    }
}
