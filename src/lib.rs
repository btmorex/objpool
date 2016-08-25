use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, SystemTime};

pub struct Pool<T, F> where F: Fn() -> T {
    create: F,
    items: Mutex<Items<T>>,
    item_available: Condvar,
    weak_self: Weak<Pool<T, F>>,
}

impl<T, F> Pool<T, F> where F: Fn() -> T {
    pub fn new(create: F) -> Arc<Pool<T, F>> {
        Pool::new_internal(create, None)
    }

    fn new_internal(create: F, max_items: Option<usize>) -> Arc<Pool<T, F>> {
        let pool = Arc::new(Pool {
            create: create,
            items: Mutex::new(Items {
                available: Vec::new(),
                count: 0,
                max: max_items,
            }),
            item_available: Condvar::new(),
            weak_self: Weak::new(),
        });
        unsafe {
            let ptr = &pool.weak_self as *const Weak<Pool<T, F>> as *mut Weak<Pool<T, F>>;
            *ptr = Arc::downgrade(&pool);
        }
        pool
    }

    pub fn get(&self) -> Item<T, F> {
        self.get_internal(None).unwrap()
    }

    pub fn get_timeout(&self, duration: Duration) -> Result<Item<T, F>, TimeoutError> {
        self.get_internal(Some(duration))
    }

    fn get_internal(&self, duration: Option<Duration>) -> Result<Item<T, F>, TimeoutError> {
        let mut items = self.items.lock().unwrap();
        let item = match items.available.pop() {
            Some(item) => item,
            None => {
                if items.count < items.max.unwrap_or(std::usize::MAX) {
                    items.count += 1;
                    drop(items);
                    (self.create)()
                } else {
                    let start = SystemTime::now();
                    while items.available.is_empty() {
                        items = match duration {
                            Some(duration) => {
                                let elapsed = match start.elapsed() {
                                    Ok(elapsed) => elapsed,
                                    Err(_) => Duration::from_secs(0),
                                };
                                if elapsed > duration {
                                    return Err(TimeoutError);
                                }
                                let (items, wait_result) =
                                    self.item_available.wait_timeout(items, duration - elapsed).unwrap();
                                if wait_result.timed_out() {
                                    return Err(TimeoutError);
                                }
                                items
                            },
                            None => self.item_available.wait(items).unwrap(),
                        }
                    }
                    items.available.pop().unwrap()
                }
            }
        };
        Ok(Item {
            item: Some(item),
            pool: self.weak_self.upgrade().unwrap(),
        })
    }

    fn put(&self, item: T) {
        self.items.lock().unwrap().available.push(item);
        self.item_available.notify_one();
    }
}

impl<T, F> Debug for Pool<T, F> where T: Debug, F: Fn() -> T {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        fmt.debug_struct("Pool")
            .field("items", &*self.items.lock().unwrap())
            .finish()
    }
}

pub struct PoolBuilder<T, F> where F: Fn() -> T {
    create: F,
    max_items: Option<usize>,
}

impl<T, F> PoolBuilder<T, F> where F: Fn() -> T {
    pub fn new(create: F) -> PoolBuilder<T, F> {
        PoolBuilder { create: create, max_items: None }
    }

    pub fn max_items(mut self, max_items: usize) -> PoolBuilder<T, F> {
        self.max_items = Some(max_items);
        self
    }

    pub fn finalize(self) -> Arc<Pool<T, F>> {
        Pool::new_internal(self.create, self.max_items)
    }
}

struct Items<T> {
    available: Vec<T>,
    count: usize,
    max: Option<usize>,
}

impl<T> Debug for Items<T> where T: Debug {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        fmt.debug_struct("Items")
            .field("available", &self.available.len())
            .field("count", &self.count)
            .field("max", &self.max)
            .finish()
    }
}

pub struct Item<T, F> where F: Fn() -> T {
    item: Option<T>,
    pool: Arc<Pool<T, F>>,
}

impl<T, F> Debug for Item<T, F> where T: Debug, F: Fn() -> T {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        fmt.debug_struct("Item")
            .field("item", &self.item)
            .finish()
    }
}

impl<T, F> Deref for Item<T, F> where F: Fn() -> T {
    type Target = T;

    fn deref(&self) -> &T {
        self.item.as_ref().unwrap()
    }
}

impl<T, F> DerefMut for Item<T, F> where F: Fn() -> T {
    fn deref_mut(&mut self) -> &mut T {
        self.item.as_mut().unwrap()
    }
}

impl<T, F> Drop for Item<T, F> where F: Fn() -> T {
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
    fn pool_builder() {
        let pool = PoolBuilder::new(|| 0).max_items(1).finalize();
        let _x = pool.get();
        assert_eq!(pool.get_timeout(Duration::from_secs(0)).err(), Some(TimeoutError));
    }

    #[test]
    fn pool_get() {
        let pool = PoolBuilder::new(|| 0).max_items(1).finalize();
        let x = pool.get();
        let p = pool.clone();
        let start = SystemTime::now();
        let handle = thread::spawn(move || {
            let _ = p.get();
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
        let pool = PoolBuilder::new(|| 0).max_items(1).finalize();
        let x = pool.get();
        let p = pool.clone();
        let start = SystemTime::now();
        let handle = thread::spawn(move || {
            let _ = p.get_timeout(Duration::from_secs(1)).unwrap();
        });
        thread::sleep(Duration::from_millis(100));
        drop(x);
        handle.join().unwrap();
        let elapsed_ms = start.elapsed().unwrap().as_millis();
        assert!(elapsed_ms >= 50 && elapsed_ms < 150);
    }

    #[test]
    fn pool_get_timeout_err() {
        let pool = PoolBuilder::new(|| 0).max_items(1).finalize();
        let _x = pool.get();
        let p = pool.clone();
        let start = SystemTime::now();
        let handle = thread::spawn(move || {
            assert_eq!(p.get_timeout(Duration::from_millis(100)).err(), Some(TimeoutError));
        });
        while start.elapsed().unwrap().as_millis() < 100 {
            pool.item_available.notify_one();
        }
        handle.join().unwrap();
        let elapsed_ms = start.elapsed().unwrap().as_millis();
        assert!(elapsed_ms >= 50 && elapsed_ms < 150);
    }

    #[test]
    fn pool_debug() {
        let pool = PoolBuilder::new(|| 0).max_items(5).finalize();
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
