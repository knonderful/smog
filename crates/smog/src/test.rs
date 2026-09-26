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
    let mut expected = vec![36, 24, 12];
    let mut gen = pin!(multiples(12));
    for item in &mut gen {
        let ex = expected.pop().expect("no more items in `expected`");
        assert_eq!(ex, item);
    }
    assert_eq!(None, gen.as_mut().advance());
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
    assert_eq!(None, gen.advance());
}
