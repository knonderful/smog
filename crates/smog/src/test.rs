use super::*;
use std::pin::pin;

fn generate_multiples(base: usize) -> Generator<impl Future<Output = ()>, usize> {
    {
        async move |mut yielder: Yielder<_>| {
            let limit = usize::MAX / base;
            for i in 1..limit {
                yielder.yeeld(i * base).await;
            }
        }
    }
    .into()
}

#[test]
fn dummy_test() {
    let mut gen = pin!(generate_multiples(12));
    assert_eq!(Some(12), gen.as_mut().next());
    assert_eq!(Some(24), gen.as_mut().next());
    assert_eq!(Some(36), gen.as_mut().next());
}
