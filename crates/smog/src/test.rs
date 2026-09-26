use super::*;
use std::pin::pin;

fn multiples(base: usize) -> Generator<impl Future<Output = ()>, usize> {
    {
        async move |mut ctx: GeneratorContext<_>| {
            for i in 1..=3 {
                ctx.emit(i * base).await;
            }
        }
    }
    .into()
}

fn multiples_with_return(base: usize) -> Generator<impl Future<Output = Return<String>>, usize> {
    {
        async move |mut ctx: GeneratorContext<_>| {
            for i in 1..=3 {
                ctx.emit(i * base).await;
            }

            format!("The base was {base}").into()
        }
    }
    .into()
}

#[test]
fn test_multiples() {
    let mut gen = pin!(multiples(12));
    assert_eq!(Some(12), gen.as_mut().next());
    assert_eq!(Some(24), gen.as_mut().next());
    assert_eq!(Some(36), gen.as_mut().next());
    assert_eq!(None, gen.as_mut().next());
    assert_eq!(None, gen.as_mut().next());
}

#[test]
fn test_multiples_iter() {
    let mut expected = vec![36, 24, 12];
    for item in pin!(multiples(12)).into_iter() {
        let ex = expected.pop().expect("no more items in `expected`");
        assert_eq!(ex, item);
    }
}

#[test]
fn test_multiples_with_return() {
    let mut gen = pin!(multiples_with_return(12));
    assert_eq!(Some(GeneratorItem::Yield(12)), gen.as_mut().next());
    assert_eq!(Some(GeneratorItem::Yield(24)), gen.as_mut().next());
    assert_eq!(Some(GeneratorItem::Yield(36)), gen.as_mut().next());
    assert_eq!(
        Some(GeneratorItem::Return("The base was 12".to_string())),
        gen.as_mut().next()
    );
    assert_eq!(None, gen.as_mut().next());
    assert_eq!(None, gen.as_mut().next());
}

#[test]
fn test_multiples_with_return_iter() {
    let mut expected = vec![
        GeneratorItem::Return("The base was 12".to_string()),
        GeneratorItem::Yield(36),
        GeneratorItem::Yield(24),
        GeneratorItem::Yield(12),
    ];
    for item in pin!(multiples_with_return(12)).into_iter() {
        let ex = expected.pop().expect("no more items in `expected`");
        assert_eq!(ex, item);
    }
}
