use std::{collections::HashMap, fmt};

use anyhow::{anyhow, Result};

pub(crate) trait ManagedProviderCore {
    fn proxy_url(&self, provider_egress_key: &str) -> Option<&str>;
    fn is_alive(&mut self) -> bool;
    fn terminate(self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderCoreError {
    Backoff,
}

impl fmt::Display for ProviderCoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("provider core startup is in backoff")
    }
}

impl std::error::Error for ProviderCoreError {}

struct Lease {
    cleanup_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AcquiredProxy {
    pub lease_id: String,
    pub proxy_url: String,
    pub cleanup_token: String,
}

pub(crate) struct ProviderCore<R> {
    idle_ttl_millis: u64,
    failure_backoff_millis: u64,
    runtime: Option<R>,
    leases: HashMap<String, Lease>,
    idle_since_millis: Option<u64>,
    retry_after_millis: Option<u64>,
}

impl<R: ManagedProviderCore> ProviderCore<R> {
    pub(crate) fn new(idle_ttl_millis: u64, failure_backoff_millis: u64) -> Self {
        Self {
            idle_ttl_millis,
            failure_backoff_millis,
            runtime: None,
            leases: HashMap::new(),
            idle_since_millis: None,
            retry_after_millis: None,
        }
    }

    pub(crate) fn acquire(
        &mut self,
        provider_egress_key: &str,
        lease_id: String,
        cleanup_token: String,
        now_millis: u64,
        create: impl FnOnce() -> Result<R>,
    ) -> Result<AcquiredProxy> {
        self.reap_idle(now_millis);
        self.remove_dead_runtime();
        if self
            .retry_after_millis
            .is_some_and(|retry_after| now_millis < retry_after)
        {
            return Err(ProviderCoreError::Backoff.into());
        }
        self.retry_after_millis = None;

        if self.runtime.is_none() {
            let runtime = match create() {
                Ok(runtime) => runtime,
                Err(error) => {
                    self.retry_after_millis =
                        Some(now_millis.saturating_add(self.failure_backoff_millis));
                    return Err(error);
                }
            };
            self.runtime = Some(runtime);
            self.idle_since_millis = Some(now_millis);
        }

        let proxy_url = self
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.proxy_url(provider_egress_key))
            .ok_or_else(|| anyhow!("provider_egress_key has no runtime listener"))?
            .to_owned();
        self.leases.insert(
            lease_id.clone(),
            Lease {
                cleanup_token: cleanup_token.clone(),
            },
        );
        self.idle_since_millis = None;
        Ok(AcquiredProxy {
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
        self.leases.remove(lease_id);
        if self.leases.is_empty() {
            self.idle_since_millis = Some(now_millis);
        }
        Ok(())
    }

    pub(crate) fn reap_idle(&mut self, now_millis: u64) {
        let expired = self.idle_since_millis.is_some_and(|idle_since| {
            now_millis.saturating_sub(idle_since) >= self.idle_ttl_millis
        });
        if expired {
            if let Some(runtime) = self.runtime.take() {
                runtime.terminate();
            }
            self.idle_since_millis = None;
        }
        if self
            .retry_after_millis
            .is_some_and(|retry_after| now_millis >= retry_after)
        {
            self.retry_after_millis = None;
        }
    }

    pub(crate) fn shutdown(&mut self) {
        self.leases.clear();
        self.idle_since_millis = None;
        self.retry_after_millis = None;
        if let Some(runtime) = self.runtime.take() {
            runtime.terminate();
        }
    }

    fn remove_dead_runtime(&mut self) {
        let dead = self
            .runtime
            .as_mut()
            .is_some_and(|runtime| !runtime.is_alive());
        if dead {
            if let Some(runtime) = self.runtime.take() {
                runtime.terminate();
            }
            self.leases.clear();
            self.idle_since_millis = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    struct FakeCore {
        proxy_urls: HashMap<String, String>,
        alive: bool,
        terminated: Arc<Mutex<usize>>,
    }

    impl ManagedProviderCore for FakeCore {
        fn proxy_url(&self, provider_egress_key: &str) -> Option<&str> {
            self.proxy_urls.get(provider_egress_key).map(String::as_str)
        }

        fn is_alive(&mut self) -> bool {
            self.alive
        }

        fn terminate(self) {
            *self.terminated.lock().unwrap() += 1;
        }
    }

    fn core(terminated: &Arc<Mutex<usize>>) -> FakeCore {
        FakeCore {
            proxy_urls: HashMap::from([
                ("node-a".to_owned(), "http://127.0.0.1:18080".to_owned()),
                ("node-b".to_owned(), "http://127.0.0.1:18081".to_owned()),
            ]),
            alive: true,
            terminated: terminated.clone(),
        }
    }

    #[test]
    fn ac_003_different_egresses_share_one_core_and_keep_independent_leases() {
        let terminated = Arc::new(Mutex::new(0));
        let mut provider = ProviderCore::new(60_000, 5_000);
        let created = Arc::new(Mutex::new(0));
        let first_created = created.clone();
        let first = provider
            .acquire("node-a", "lease-a".into(), "token-a".into(), 10, || {
                *first_created.lock().unwrap() += 1;
                Ok(core(&terminated))
            })
            .unwrap();
        let second = provider
            .acquire("node-b", "lease-b".into(), "token-b".into(), 20, || {
                panic!("a second egress must reuse the provider core")
            })
            .unwrap();

        assert_eq!(*created.lock().unwrap(), 1);
        assert_ne!(first.proxy_url, second.proxy_url);
        provider.release("lease-a", "token-a", 30).unwrap();
        assert!(provider.release("lease-b", "token-a", 40).is_err());
        provider.release("lease-b", "token-b", 50).unwrap();
        assert_eq!(*terminated.lock().unwrap(), 0);
    }

    #[test]
    fn ac_004_idle_ttl_and_shutdown_terminate_the_single_core() {
        let terminated = Arc::new(Mutex::new(0));
        let mut provider = ProviderCore::new(100, 5_000);
        provider
            .acquire("node-a", "lease-a".into(), "token-a".into(), 10, || {
                Ok(core(&terminated))
            })
            .unwrap();
        provider.release("lease-a", "token-a", 20).unwrap();
        provider.reap_idle(119);
        assert_eq!(*terminated.lock().unwrap(), 0);
        provider.reap_idle(120);
        assert_eq!(*terminated.lock().unwrap(), 1);

        provider
            .acquire("node-b", "lease-b".into(), "token-b".into(), 130, || {
                Ok(core(&terminated))
            })
            .unwrap();
        provider.shutdown();
        assert_eq!(*terminated.lock().unwrap(), 2);
    }

    #[test]
    fn ac_005_failed_start_is_backed_off_then_recovers() {
        let terminated = Arc::new(Mutex::new(0));
        let mut provider = ProviderCore::<FakeCore>::new(100, 50);
        assert!(provider
            .acquire("node-a", "lease-a".into(), "token-a".into(), 10, || {
                Err(anyhow!("controlled failure"))
            })
            .is_err());
        let error = provider
            .acquire("node-b", "lease-b".into(), "token-b".into(), 11, || {
                panic!("backoff must suppress a second startup")
            })
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<ProviderCoreError>(),
            Some(&ProviderCoreError::Backoff)
        );
        assert!(provider
            .acquire("node-b", "lease-c".into(), "token-c".into(), 60, || {
                Ok(core(&terminated))
            })
            .is_ok());
    }
}
