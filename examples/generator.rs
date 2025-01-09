use smog::portal::output::{OutBack, OutEvent};
use smog::{create_driver, CoroPoll};
use std::pin::pin;

async fn fibonacci(mut portal: OutBack<u8>) {
    // Fibonacci is a good example of the power of coroutines, since this "anomaly" at the start would require a lot
    // more complex code with "normal" code.
    for _ in 0..2 {
        portal.send(0).await;
    }

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
    let mut driver = pin!(create_driver(fibonacci));

    while let CoroPoll::Event(portal_event) = driver.poll() {
        match portal_event {
            OutEvent::Yielded(val) => print!(" {val}"),
        }
    }
    println!();
}
