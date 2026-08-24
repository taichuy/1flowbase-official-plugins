use std::{collections::HashMap, fmt};

use anyhow::{anyhow, Result};

pub(crate) trait ManagedRuntime {
    fn proxy_url(&self) -> &str;
    fn is_alive(&mut self) -> bool;
    fn terminate(self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PoolError {
    CapacityExhausted,
    Backoff,
}

impl fmt::Display for PoolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CapacityExhausted => "runtime pool capacity is exhausted",
            Self::Backoff => "runtime startup is in backoff",
        })
    }
}

impl std::error::Error for PoolError {}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeState {
    Ready { reference_count: usize },
    Idle { since_millis: u64 },
}

struct RuntimeSlot<R> {
    runtime: R,
    state: RuntimeState,
    last_used_millis: u64,
}

struct Lease {
    key: String,
    cleanup_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AcquiredRuntime {
    pub lease_id: String,
    pub proxy_url: String,
    pub cleanup_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FailedRuntime {
    retry_after_millis: u64,
}

pub(crate) struct RuntimePool<R> {
    capacity: usize,
    idle_ttl_millis: u64,
    failure_backoff_millis: u64,
    runtimes: HashMap<String, RuntimeSlot<R>>,
    leases: HashMap<String, Lease>,
    failures: HashMap<String, FailedRuntime>,
}

impl<R: ManagedRuntime> RuntimePool<R> {
    pub(crate) fn new(capacity: usize, idle_ttl_millis: u64, failure_backoff_millis: u64) -> Self {
        assert!(capacity > 0, "runtime pool capacity must be positive");
        Self {
            capacity,
            idle_ttl_millis,
            failure_backoff_millis,
            runtimes: HashMap::new(),
            leases: HashMap::new(),
            failures: HashMap::new(),
        }
    }

    pub(crate) fn acquire(
        &mut self,
        key: &str,
        lease_id: String,
        cleanup_token: String,
        now_millis: u64,
        create: impl FnOnce() -> Result<R>,
    ) -> Result<AcquiredRuntime> {
        self.reap_idle(now_millis);
        self.remove_dead_runtime(key);

        if self
            .failures
            .get(key)
            .is_some_and(|failure| now_millis < failure.retry_after_millis)
        {
            return Err(PoolError::Backoff.into());
        }
        self.failures.remove(key);

        if !self.runtimes.contains_key(key) {
            self.evict_least_recently_used_idle();
            if self.runtimes.len() >= self.capacity {
                return Err(PoolError::CapacityExhausted.into());
            }
            let runtime = match create() {
                Ok(runtime) => runtime,
                Err(error) => {
                    self.failures.insert(
                        key.to_owned(),
                        FailedRuntime {
                            retry_after_millis: now_millis
                                .saturating_add(self.failure_backoff_millis),
                        },
                    );
                    return Err(error);
                }
            };
            self.runtimes.insert(
                key.to_owned(),
                RuntimeSlot {
                    runtime,
                    state: RuntimeState::Idle {
                        since_millis: now_millis,
                    },
                    last_used_millis: now_millis,
                },
            );
        }

        let slot = self
            .runtimes
            .get_mut(key)
            .expect("runtime was inserted or already present");
        let reference_count = match slot.state {
            RuntimeState::Ready { reference_count } => reference_count.saturating_add(1),
            RuntimeState::Idle { .. } => 1,
        };
        slot.state = RuntimeState::Ready { reference_count };
        slot.last_used_millis = now_millis;
        let proxy_url = slot.runtime.proxy_url().to_owned();
        self.leases.insert(
            lease_id.clone(),
            Lease {
                key: key.to_owned(),
                cleanup_token: cleanup_token.clone(),
            },
        );
        Ok(AcquiredRuntime {
            lease_id,
            proxy_url,
            cleanup_token,
        })
    }

    pub(crate) fn release(
        &mut self,
        lease_id: &str,
        cleanup_token: &str,
        now_millis: u64,
    ) -> Result<()> {
        let lease = self
            .leases
            .get(lease_id)
            .ok_or_else(|| anyhow!("unknown lease_id"))?;
        if lease.cleanup_token != cleanup_token {
            return Err(anyhow!("cleanup_token does not match lease_id"));
        }
        let lease = self
            .leases
            .remove(lease_id)
            .expect("validated lease remains present");
        let slot = self
            .runtimes
            .get_mut(&lease.key)
            .ok_or_else(|| anyhow!("lease runtime is unavailable"))?;
        let RuntimeState::Ready { reference_count } = slot.state else {
            return Err(anyhow!("lease runtime is not active"));
        };
        slot.last_used_millis = now_millis;
        slot.state = if reference_count == 1 {
            RuntimeState::Idle {
                since_millis: now_millis,
            }
        } else {
            RuntimeState::Ready {
                reference_count: reference_count - 1,
            }
        };
        Ok(())
    }

    pub(crate) fn reap_idle(&mut self, now_millis: u64) {
        let expired = self
            .runtimes
            .iter()
            .filter_map(|(key, slot)| match slot.state {
                RuntimeState::Idle { since_millis }
                    if now_millis.saturating_sub(since_millis) >= self.idle_ttl_millis =>
                {
                    Some(key.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for key in expired {
            if let Some(slot) = self.runtimes.remove(&key) {
                slot.runtime.terminate();
            }
        }
        self.failures
            .retain(|_, failure| now_millis < failure.retry_after_millis);
    }

    pub(crate) fn shutdown(&mut self) {
        self.leases.clear();
        self.failures.clear();
        for (_, slot) in self.runtimes.drain() {
            slot.runtime.terminate();
        }
    }

    fn remove_dead_runtime(&mut self, key: &str) {
        let dead = self
            .runtimes
            .get_mut(key)
            .is_some_and(|slot| !slot.runtime.is_alive());
        if dead {
            if let Some(slot) = self.runtimes.remove(key) {
                slot.runtime.terminate();
            }
            self.leases.retain(|_, lease| lease.key != key);
        }
    }

    fn evict_least_recently_used_idle(&mut self) {
        if self.runtimes.len() < self.capacity {
            return;
        }
        let candidate = self
            .runtimes
            .iter()
            .filter(|(_, slot)| matches!(slot.state, RuntimeState::Idle { .. }))
            .min_by_key(|(_, slot)| slot.last_used_millis)
            .map(|(key, _)| key.clone());
        if let Some(key) = candidate {
            if let Some(slot) = self.runtimes.remove(&key) {
                slot.runtime.terminate();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    struct FakeRuntime {
        proxy_url: String,
        alive: bool,
        terminated: Arc<Mutex<Vec<String>>>,
        marker: String,
    }

    impl ManagedRuntime for FakeRuntime {
        fn proxy_url(&self) -> &str {
            &self.proxy_url
        }

        fn is_alive(&mut self) -> bool {
            self.alive
        }

        fn terminate(self) {
            self.terminated.lock().unwrap().push(self.marker);
        }
    }

    fn runtime(marker: &str, terminated: &Arc<Mutex<Vec<String>>>) -> FakeRuntime {
        FakeRuntime {
            proxy_url: format!("http://127.0.0.1/{marker}"),
            alive: true,
            terminated: terminated.clone(),
            marker: marker.to_owned(),
        }
    }

    #[test]
    fn ac_001_ac_002_reuses_one_runtime_while_keeping_leases_independent() {
        let terminated = Arc::new(Mutex::new(Vec::new()));
        let mut pool = RuntimePool::new(4, 60_000, 5_000);
        let first = pool
            .acquire("node-a", "lease-a".into(), "token-a".into(), 10, || {
                Ok(runtime("node-a", &terminated))
            })
            .unwrap();
        let second = pool
            .acquire("node-a", "lease-b".into(), "token-b".into(), 20, || {
                panic!("a ready runtime must be reused")
            })
            .unwrap();

        assert_eq!(first.proxy_url, second.proxy_url);
        assert_ne!(first.lease_id, second.lease_id);
        pool.release("lease-a", "token-a", 30).unwrap();
        assert!(pool.release("lease-b", "token-a", 40).is_err());
        pool.release("lease-b", "token-b", 50).unwrap();
        assert!(terminated.lock().unwrap().is_empty());
    }

    #[test]
    fn ac_003_ac_004_evicts_only_idle_lru_and_never_an_active_runtime() {
        let terminated = Arc::new(Mutex::new(Vec::new()));
        let mut pool = RuntimePool::new(2, 60_000, 5_000);
        pool.acquire("active", "lease-a".into(), "token-a".into(), 1, || {
            Ok(runtime("active", &terminated))
        })
        .unwrap();
        pool.acquire("idle", "lease-b".into(), "token-b".into(), 2, || {
            Ok(runtime("idle", &terminated))
        })
        .unwrap();
        pool.release("lease-b", "token-b", 3).unwrap();
        pool.acquire("new", "lease-c".into(), "token-c".into(), 4, || {
            Ok(runtime("new", &terminated))
        })
        .unwrap();
        assert_eq!(terminated.lock().unwrap().as_slice(), &["idle"]);
        assert!(pool
            .acquire("fourth", "lease-d".into(), "token-d".into(), 5, || {
                Ok(runtime("fourth", &terminated))
            })
            .is_err());
    }

    #[test]
    fn ac_004_reaps_idle_after_ttl_and_shutdown_cleans_active_runtime() {
        let terminated = Arc::new(Mutex::new(Vec::new()));
        let mut pool = RuntimePool::new(2, 100, 5_000);
        pool.acquire("idle", "lease-a".into(), "token-a".into(), 10, || {
            Ok(runtime("idle", &terminated))
        })
        .unwrap();
        pool.release("lease-a", "token-a", 20).unwrap();
        pool.reap_idle(119);
        assert!(terminated.lock().unwrap().is_empty());
        pool.reap_idle(120);
        assert_eq!(terminated.lock().unwrap().as_slice(), &["idle"]);

        pool.acquire("active", "lease-b".into(), "token-b".into(), 130, || {
            Ok(runtime("active", &terminated))
        })
        .unwrap();
        pool.shutdown();
        assert_eq!(terminated.lock().unwrap().as_slice(), &["idle", "active"]);
    }

    #[test]
    fn ac_005_backs_off_a_failed_runtime_without_restarting_it() {
        let mut pool = RuntimePool::<FakeRuntime>::new(2, 100, 50);
        let attempts = Arc::new(Mutex::new(0));
        let first_attempts = attempts.clone();
        assert!(pool
            .acquire("failed", "lease-a".into(), "token-a".into(), 10, || {
                *first_attempts.lock().unwrap() += 1;
                Err(anyhow!("controlled startup failure"))
            })
            .is_err());
        let second_attempts = attempts.clone();
        assert!(pool
            .acquire("failed", "lease-b".into(), "token-b".into(), 20, || {
                *second_attempts.lock().unwrap() += 1;
                panic!("backoff must suppress a second startup")
            })
            .is_err());
        assert_eq!(*attempts.lock().unwrap(), 1);
    }

    #[test]
    fn ac_003_ac_009_twenty_distinct_requests_never_exceed_the_runtime_capacity() {
        let terminated = Arc::new(Mutex::new(Vec::new()));
        let created = Arc::new(Mutex::new(0_usize));
        let mut pool = RuntimePool::new(4, 60_000, 5_000);
        let mut acquired = Vec::new();
        let mut rejected = 0;

        for index in 0..20 {
            let created = created.clone();
            let terminated = terminated.clone();
            let result = pool.acquire(
                &format!("node-{index}"),
                format!("lease-{index}"),
                format!("token-{index}"),
                index,
                || {
                    *created.lock().unwrap() += 1;
                    Ok(runtime(&format!("node-{index}"), &terminated))
                },
            );
            match result {
                Ok(lease) => acquired.push(lease),
                Err(error) => {
                    assert_eq!(
                        error.downcast_ref::<PoolError>(),
                        Some(&PoolError::CapacityExhausted)
                    );
                    rejected += 1;
                }
            }
        }

        assert_eq!(*created.lock().unwrap(), 4);
        assert_eq!(acquired.len(), 4);
        assert_eq!(rejected, 16);
        assert!(terminated.lock().unwrap().is_empty());
        pool.shutdown();
        assert_eq!(terminated.lock().unwrap().len(), 4);
    }
}
