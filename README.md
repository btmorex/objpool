# objpool

Thread-safe generic object pool

[![Build Status](https://travis-ci.org/btmorex/objpool.svg?branch=master)](https://travis-ci.org/btmorex/objpool) [![Coverage Status](https://coveralls.io/repos/github/btmorex/objpool/badge.svg?branch=master)](https://coveralls.io/github/btmorex/objpool?branch=master)

* [Documentation](https://btmorex.github.io/objpool/objpool/index.html)
* [crates.io](https://crates.io/crates/objpool)

## Examples

```rust
use objpool::Pool;
use std::thread;

let pool = Pool::with_capacity(5, || 0);
let mut handles = Vec::new();
for _ in 0..10 {
    let pool = pool.clone();
    handles.push(thread::spawn(move || {
        for _ in 0..1000 {
            *pool.get() += 1;
        }
    }));
}

for handle in handles {
    handle.join().unwrap();
}
assert_eq!(*pool.get() + *pool.get() + *pool.get() + *pool.get() + *pool.get(), 10000);
```
