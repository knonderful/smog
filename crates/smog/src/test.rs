use super::*;
use std::pin::pin;

fn multiples(base: usize) -> Generator<impl Future<Output = ()>, usize> {
    generator(async move |mut ctx: GeneratorContext<_>| {
        for i in 1..=3 {
            ctx.yield_value(i * base).await;
        }
    })
}

fn multiples_with_return(base: usize) -> Generator<impl Future<Output = Return<String>>, usize> {
    generator(async move |mut ctx: GeneratorContext<_>| {
        for i in 1..=3 {
            ctx.yield_value(i * base).await;
        }

        format!("The base was {base}").into()
    })
}

fn infinite(base: usize) -> Generator<impl Future<Output = Never>, usize> {
    generator(async move |mut ctx: GeneratorContext<_>| {
        let mut i = base;
        loop {
            ctx.yield_value(i).await;
            i %= 3 * base;
            i += base;
        }
    })
}

#[test]
fn test_multiples() {
    let mut expected = vec![36, 24, 12];
    let mut gen = pin!(multiples(12));
    for item in &mut gen {
        let ex = expected.pop().expect("no more items in `expected`");
        assert_eq!(ex, item);
    }
    assert_eq!(None, gen.as_mut().iter_next());
}

#[test]
fn test_multiples_with_return() {
    let mut expected = vec![
        GeneratorItem::Return("The base was 12".to_string()),
        GeneratorItem::Yield(36),
        GeneratorItem::Yield(24),
        GeneratorItem::Yield(12),
    ];
    let mut gen = pin!(multiples_with_return(12));
    for item in &mut gen {
        let ex = expected.pop().expect("no more items in `expected`");
        assert_eq!(ex, item);
    }
    assert_eq!(None, gen.iter_next());
}

#[test]
fn test_infinite() {
    let mut gen = pin!(infinite(12));
    for _ in 0..10 {
        assert_eq!(12, gen.as_mut().next_value());
        assert_eq!(24, gen.as_mut().next_value());
        assert_eq!(36, gen.as_mut().next_value());
    }
}
