use smog::portal::output::{OutBack, OutEvent};
use smog::{create_driver, CoroPoll};
use std::pin::pin;

async fn generate(mut portal: OutBack<u8>) {
    let mut a: u8 = 0;
    let mut b: u8 = 1;

    loop {
        // Abort when we overflow
        let Some(c) = a.checked_add(b) else {
            return;
        };

        portal.send(c).await;

        a = b;
        b = c;
    }
}

fn main() {
    // The driver is what allows us to both advance the state machine and receive events from the coroutine.
    let mut driver = pin!(create_driver(generate));

    while let CoroPoll::Event(portal_event) = driver.poll() {
        match portal_event {
            OutEvent::Yielded(val) => print!(" {val}"),
        }
    }
    println!();
}
