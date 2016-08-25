# objpool

Thread-safe generic object pool

[![Build Status](https://travis-ci.org/btmorex/objpool.svg?branch=master)](https://travis-ci.org/btmorex/objpool) [![Coverage Status](https://coveralls.io/repos/github/btmorex/objpool/badge.svg?branch=master)](https://coveralls.io/github/btmorex/objpool?branch=master)

* [Documentation](https://btmorex.github.io/objpool/objpool/index.html)
* [crates.io](https://crates.io/crates/objpool)

## Examples

```rust
use objpool::PoolBuilder;
use std::thread;

let pool = PoolBuilder::new(|| 0).max_items(5).finalize();
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
