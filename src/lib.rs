//! # Examples
//!
//! ```
//! use objpool::Pool;
//! use std::thread;
//!
//! let pool = Pool::with_capacity(5, || 0);
//! let mut handles = Vec::new();
//! for _ in 0..10 {
//!     let pool = pool.clone();
//!     handles.push(thread::spawn(move || {
//!         for _ in 0..1000 {
//!             *pool.get() += 1;
//!         }
//!     }));
//! }
//!
//! for handle in handles {
//!     handle.join().unwrap();
//! }
//! assert_eq!(*pool.get() + *pool.get() + *pool.get() + *pool.get() + *pool.get(), 10000);
//! ```

use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, SystemTime};

pub struct Pool<T> {
    constructor: Box<Fn() -> T + Send + Sync + 'static>,
    items: Mutex<Items<T>>,
    item_available: Condvar,
    weak_self: Weak<Pool<T>>,
}

impl<T> Pool<T> {
    pub fn new<C>(constructor: C) -> Arc<Pool<T>>
        where C: Fn() -> T + Send + Sync + 'static {

        Pool::with_capacity(std::usize::MAX, constructor)
    }

    pub fn with_capacity<C>(capacity: usize, constructor: C) -> Arc<Pool<T>>
        where C: Fn() -> T + Send + Sync + 'static {

        let pool = Arc::new(Pool {
            constructor: Box::new(constructor),
            items: Mutex::new(Items {
                available: Vec::new(),
                count: 0,
                max: Some(capacity),
            }),
            item_available: Condvar::new(),
            weak_self: Weak::new(),
        });
        unsafe {
            let ptr = &pool.weak_self as *const Weak<Pool<T>> as *mut Weak<Pool<T>>;
            *ptr = Arc::downgrade(&pool);
        }
        pool
    }

    pub fn get(&self) -> Item<T> {
        self.get_impl(None).unwrap()
    }

    pub fn get_timeout(&self, duration: Duration) -> Result<Item<T>, TimeoutError> {
        self.get_impl(Some(duration))
    }

    fn get_impl(&self, duration: Option<Duration>) -> Result<Item<T>, TimeoutError> {
        let mut items = self.items.lock().unwrap();

        if let Some(item) = items.available.pop() {
            return Ok(self.wrap(item));
        }

        if items.count < items.max.unwrap_or(std::usize::MAX) {
            items.count += 1;
            drop(items);
            return Ok(self.wrap((self.constructor)()));
        }

        if duration.is_some() {
            let duration = duration.unwrap();
            let start = SystemTime::now();
            while items.available.is_empty() {
                let elapsed = start.elapsed().unwrap_or(Duration::from_secs(0));
                if elapsed >= duration {
                    return Err(TimeoutError);
                }
                items = self.item_available.wait_timeout(items, duration - elapsed).unwrap().0;
            }
        } else {
            while items.available.is_empty() {
                items = self.item_available.wait(items).unwrap();
            }
        }

        Ok(self.wrap(items.available.pop().unwrap()))
    }

    fn wrap(&self, item: T) -> Item<T> {
        Item {
            item: Some(item),
            pool: self.weak_self.upgrade().unwrap(),
        }
    }

    fn put(&self, item: T) {
        self.items.lock().unwrap().available.push(item);
        self.item_available.notify_one();
    }
}

impl<T> Debug for Pool<T> {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        fmt.debug_struct("Pool")
            .field("items", &*self.items.lock().unwrap())
            .finish()
    }
}

struct Items<T> {
    available: Vec<T>,
    count: usize,
    max: Option<usize>,
}

impl<T> Debug for Items<T> {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        fmt.debug_struct("Items")
            .field("available", &self.available.len())
            .field("count", &self.count)
            .field("max", &self.max)
            .finish()
    }
}

pub struct Item<T> {
    item: Option<T>,
    pool: Arc<Pool<T>>,
}

impl<T> Debug for Item<T> where T: Debug {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        fmt.debug_struct("Item")
            .field("item", &self.item)
            .finish()
    }
}

impl<T> Deref for Item<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.item.as_ref().unwrap()
    }
}

impl<T> DerefMut for Item<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.item.as_mut().unwrap()
    }
}

impl<T> Drop for Item<T> {
    fn drop(&mut self) {
        self.pool.put(self.item.take().unwrap());
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct TimeoutError;

impl Display for TimeoutError {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        fmt.write_str("wait timed out")
    }
}

impl Error for TimeoutError {
    fn description(&self) -> &str {
        "wait timed out"
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::time::{Duration, SystemTime};
    use std::thread;
    use super::*;

    trait AsMillis {
        fn as_millis(&self) -> u64;
    }

    impl AsMillis for Duration {
        fn as_millis(&self) -> u64 {
            self.as_secs() * 1000 + self.subsec_nanos() as u64 / 1000000
        }
    }

    #[test]
    fn pool_get() {
        let pool = Pool::with_capacity(1, || 0);
        let x = pool.get();
        let start = SystemTime::now();
        let handle = thread::spawn(move || {
            let _ = pool.get();
        });
        thread::sleep(Duration::from_millis(100));
        drop(x);
        handle.join().unwrap();
        let elapsed_ms = start.elapsed().unwrap().as_millis();
        assert!(elapsed_ms >= 50 && elapsed_ms < 150);
    }

    #[test]
    fn pool_get_mut() {
        let pool = Pool::new(|| 0);
        *pool.get() = 4;
        assert_eq!(*pool.get(), 4);
    }

    #[test]
    fn pool_get_timeout_ok() {
        let pool = Pool::with_capacity(1, || 0);
        let x = pool.get();
        let start = SystemTime::now();
        let handle = thread::spawn(move || {
            let _ = pool.get_timeout(Duration::from_secs(1)).unwrap();
        });
        thread::sleep(Duration::from_millis(100));
        drop(x);
        handle.join().unwrap();
        let elapsed_ms = start.elapsed().unwrap().as_millis();
        assert!(elapsed_ms >= 50 && elapsed_ms < 150);
    }

    #[test]
    fn pool_get_timeout_err() {
        let pool = Pool::with_capacity(1, || 0);
        let _x = pool.get();
        let start = SystemTime::now();
        let handle = {
            let pool = pool.clone();
            thread::spawn(move || {
                assert_eq!(pool.get_timeout(Duration::from_millis(100)).err(), Some(TimeoutError));
            })
        };
        while start.elapsed().unwrap().as_millis() < 100 {
            pool.item_available.notify_one();
        }
        handle.join().unwrap();
        let elapsed_ms = start.elapsed().unwrap().as_millis();
        assert!(elapsed_ms >= 50 && elapsed_ms < 150);
    }

    #[test]
    fn pool_debug() {
        let pool = Pool::with_capacity(5, || 0);
        assert_eq!(format!("{:?}", pool),
            "Pool { items: Items { available: 0, count: 0, max: Some(5) } }");
        let x = pool.get();
        let _y = pool.get();
        let _z = pool.get();
        drop(x);
        assert_eq!(format!("{:?}", pool),
            "Pool { items: Items { available: 1, count: 3, max: Some(5) } }");
    }

    #[test]
    fn item_debug() {
        let pool = Pool::new(|| 0);
        let x = pool.get();
        assert_eq!(format!("{:?}", x), "Item { item: Some(0) }");
    }

    #[test]
    fn timeout_error() {
        assert_eq!(format!("{}", TimeoutError), "wait timed out");
        assert_eq!(TimeoutError.description(), "wait timed out");
    }
}
