//! This example shows how to stream data into a coroutine. The final product is the result of the coroutine.

use smog::portal::input::{InBack, InEvent, InFront};
use smog::{driver, CoroPoll, Driver, Storage};
use std::future::Future;
use std::pin::Pin;

enum StreamingEvent<'a> {
    OpenDocument { name: String, content_type: String },
    Line(&'a str),
    CloseDocument,
}

struct Document {
    name: String,
    content_type: String,
    lines: Vec<String>,
}

impl Document {
    fn new(name: String, content_type: String) -> Self {
        Self {
            name,
            content_type,
            lines: Vec::new(),
        }
    }
}

async fn highlight(mut portal: InBack<StreamingEvent<'_>>, pattern: &str) -> Result<Document, &'static str> {
    use StreamingEvent::*;

    let OpenDocument { name, content_type } = portal.receive().await else {
        return Err("Expected to start with OpenDocument.");
    };

    let replacement = format!("*{pattern}*");

    let mut doc = Document::new(name, content_type);
    loop {
        match portal.receive().await {
            OpenDocument { .. } => return Err("Expected OpenDocument only at the start."),
            Line(line) => doc.lines.push(line.replace(pattern, &replacement)),
            CloseDocument => break,
        }
    }

    Ok(doc)
}

fn drive<'a, Fut>(driver: &mut Driver<Pin<Box<Storage<InFront<StreamingEvent<'a>>, Fut>>>, Fut::Output>)
where
    Fut: Future<Output = Result<Document, &'static str>> + 'a,
{
    match driver.poll() {
        CoroPoll::Event(InEvent::Awaiting) => {}
        CoroPoll::Result(result) => match result {
            Ok(_) => panic!("Unexpectedly got a result."),
            Err(err) => panic!("Error: {err}"),
        },
    }
}

fn main() {
    const INPUT: &str = r#"Why Rust?
Performance
Rust is blazingly fast and memory-efficient: with no runtime or garbage
collector, it can power performance-critical  services, run on embedded devices,
and easily integrate with other languages.

Reliability
Rust’s rich type system and ownership model guarantee memory-safety and
thread-safety — enabling you to eliminate many classes of bugs at compile-time.

Productivity
Rust has great documentation, a friendly compiler with useful error messages,
and top-notch tooling — an integrated package manager and build tool, smart
multi-editor support with auto-completion and type inspections, an auto-
formatter, and more."#;

    // The driver is what allows us to both advance the state machine and receive events from the coroutine.
    let driver = &mut driver(|back| highlight(back, "Rust"));

    // We must drive the coroutine to bring it into the first awaiting state.
    drive(driver);
    driver.portal().provide(StreamingEvent::OpenDocument {
        name: "https://www.rust-lang.org/".to_string(),
        content_type: "text/html".to_string(),
    });

    drive(driver);
    for line in INPUT.lines() {
        driver.portal().provide(StreamingEvent::Line(line));
        drive(driver);
    }

    driver.portal().provide(StreamingEvent::CloseDocument);

    // Drive to completion
    let CoroPoll::Result(result) = driver.poll() else {
        panic!("Expected a result, but got another event.");
    };

    match result {
        Ok(doc) => {
            println!("Name:         {}", doc.name);
            println!("Content-Type: {}", doc.content_type);
            println!("--------------------------------------------------------------------------------");
            for line in doc.lines {
                println!("{line}");
            }
            println!("--------------------------------------------------------------------------------");
        }
        Err(err) => panic!("Error: {err}"),
    }
}
