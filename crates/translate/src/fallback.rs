//! Online engine circuit breaker and automatic fallback to offline.
//!
//! Network requests to cloud translation services or remote LLMs can suffer high latency,
//! rate limits, connection drops, or outages. A circuit breaker tracks failures and temporarily
//! suspends online calls when a threshold is breached, falling back seamlessly to offline local
//! models so the user experience never freezes.

use lumen_language::Language;
use serde::{Deserialize, Serialize};

use crate::engine::{EngineKind, TranslationEngine, TranslationError, TranslationRequest, TranslationResponse};

/// Operating state of the circuit breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitState {
    /// Normal operation: online requests are allowed.
    Closed,
    /// Failure threshold exceeded: online requests are blocked, fallback used immediately.
    Open,
    /// Recovery probation: probe requests test if the online service has recovered.
    HalfOpen,
}

/// A circuit breaker safeguarding against cascading network failures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreaker {
    /// Number of consecutive failures before the breaker trips open.
    pub failure_threshold: usize,
    /// Cooldown time in milliseconds before attempting recovery.
    pub recovery_cooldown_ms: u64,
    /// Number of successful probes needed to re-close the circuit from half-open.
    pub success_threshold: usize,
    state: CircuitState,
    consecutive_failures: usize,
    consecutive_successes: usize,
    last_failure_ms: u64,
}

impl CircuitBreaker {
    /// Creates a circuit breaker with specified failure threshold and cooldown.
    pub fn new(failure_threshold: usize, recovery_cooldown_ms: u64) -> Self {
        Self {
            failure_threshold: failure_threshold.max(1),
            recovery_cooldown_ms,
            success_threshold: 2,
            state: CircuitState::Closed,
            consecutive_failures: 0,
            consecutive_successes: 0,
            last_failure_ms: 0,
        }
    }

    /// Default policy: 3 consecutive failures trip the breaker; 15s cooldown.
    pub fn standard() -> Self {
        Self::new(3, 15_000)
    }

    /// Current circuit state.
    pub fn state(&self) -> CircuitState {
        self.state
    }

    /// Evaluates whether a new primary request should be attempted at timestamp `now_ms`.
    pub fn allow_request(&mut self, now_ms: u64) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                if now_ms.saturating_sub(self.last_failure_ms) >= self.recovery_cooldown_ms {
                    self.state = CircuitState::HalfOpen;
                    self.consecutive_successes = 0;
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// Records a successful primary call.
    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        if self.state == CircuitState::HalfOpen {
            self.consecutive_successes += 1;
            if self.consecutive_successes >= self.success_threshold {
                self.state = CircuitState::Closed;
            }
        }
    }

    /// Records a failed primary call, potentially tripping the circuit open.
    pub fn record_failure(&mut self, now_ms: u64) {
        self.consecutive_failures += 1;
        self.last_failure_ms = now_ms;
        if self.consecutive_failures >= self.failure_threshold || self.state == CircuitState::HalfOpen {
            self.state = CircuitState::Open;
        }
    }

    /// Manually resets breaker to closed state.
    pub fn reset(&mut self) {
        self.state = CircuitState::Closed;
        self.consecutive_failures = 0;
        self.consecutive_successes = 0;
        self.last_failure_ms = 0;
    }
}

/// Composite engine coordinating primary (e.g. online) and secondary (e.g. offline) engines.
pub struct FallbackEngine<P: TranslationEngine, S: TranslationEngine> {
    primary: P,
    secondary: S,
    breaker: CircuitBreaker,
    clock_ms: u64,
}

impl<P: TranslationEngine, S: TranslationEngine> FallbackEngine<P, S> {
    /// Wraps primary and secondary engines behind a circuit breaker.
    pub fn new(primary: P, secondary: S, breaker: CircuitBreaker) -> Self {
        Self {
            primary,
            secondary,
            breaker,
            clock_ms: 0,
        }
    }

    /// Updates internal simulated or actual clock.
    pub fn advance_time(&mut self, delta_ms: u64) {
        self.clock_ms = self.clock_ms.saturating_add(delta_ms);
    }

    /// Reference to the breaker for inspection.
    pub fn circuit_breaker(&self) -> &CircuitBreaker {
        &self.breaker
    }

    /// Mutable reference to the breaker for manual transitions or tests.
    pub fn circuit_breaker_mut(&mut self) -> &mut CircuitBreaker {
        &mut self.breaker
    }
}

impl<P: TranslationEngine, S: TranslationEngine> TranslationEngine for FallbackEngine<P, S> {
    fn translate(&mut self, req: &TranslationRequest) -> Result<TranslationResponse, TranslationError> {
        if self.breaker.allow_request(self.clock_ms) {
            match self.primary.translate(req) {
                Ok(response) => {
                    self.breaker.record_success();
                    return Ok(response);
                }
                Err(_err) => {
                    self.breaker.record_failure(self.clock_ms);
                }
            }
        }

        // Fallback to secondary engine
        self.secondary.translate(req)
    }

    fn is_available(&self, source: Language, target: Language) -> bool {
        self.primary.is_available(source, target) || self.secondary.is_available(source, target)
    }

    fn engine_kind(&self) -> EngineKind {
        EngineKind::Offline
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{StubTranslationEngine, TranslateItem};

    struct FailingEngine;
    impl TranslationEngine for FailingEngine {
        fn translate(&mut self, _req: &TranslationRequest) -> Result<TranslationResponse, TranslationError> {
            Err(TranslationError::Network("connection refused".to_owned()))
        }
        fn is_available(&self, _s: Language, _t: Language) -> bool {
            true
        }
        fn engine_kind(&self) -> EngineKind {
            EngineKind::Online
        }
    }

    #[test]
    fn trips_circuit_and_falls_back_to_secondary() {
        let primary = FailingEngine;
        let secondary = StubTranslationEngine::new();
        let breaker = CircuitBreaker::new(2, 5_000);
        let mut fallback = FallbackEngine::new(primary, secondary, breaker);

        let request = TranslationRequest {
            items: vec![TranslateItem {
                id: 1,
                text: "Quit".to_owned(),
                kind: None,
            }],
            source_language: Language::English,
            target_language: Language::Russian,
            context: None,
            app_id: None,
        };

        // First call fails primary -> secondary succeeds
        let res1 = fallback.translate(&request).unwrap();
        assert_eq!(res1.items[0].translated, "Выход");
        assert_eq!(fallback.circuit_breaker().state(), CircuitState::Closed);

        // Second call fails primary -> breaker trips OPEN
        let res2 = fallback.translate(&request).unwrap();
        assert_eq!(res2.items[0].translated, "Выход");
        assert_eq!(fallback.circuit_breaker().state(), CircuitState::Open);

        // Advance past cooldown -> enters HalfOpen
        fallback.advance_time(5_001);
        assert!(fallback.circuit_breaker_mut().allow_request(5_001));
    }

    #[test]
    fn a_failure_while_half_open_reopens_the_circuit() {
        let mut breaker = CircuitBreaker::new(3, 1_000);
        breaker.record_failure(0);
        breaker.record_failure(0);
        breaker.record_failure(0);
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(breaker.allow_request(1_000));
        assert_eq!(breaker.state(), CircuitState::HalfOpen);
        breaker.record_failure(1_000);
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.allow_request(1_500));
    }

    #[test]
    fn two_successes_while_half_open_close_the_circuit() {
        let mut breaker = CircuitBreaker::new(1, 100);
        breaker.record_failure(0);
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(breaker.allow_request(100));
        assert_eq!(breaker.state(), CircuitState::HalfOpen);
        breaker.record_success();
        assert_eq!(breaker.state(), CircuitState::HalfOpen);
        breaker.record_success();
        assert_eq!(breaker.state(), CircuitState::Closed);
    }

    #[test]
    fn an_open_circuit_does_not_call_the_primary() {
        struct PanicIfCalledAgain {
            calls: usize,
        }
        impl TranslationEngine for PanicIfCalledAgain {
            fn translate(&mut self, _req: &TranslationRequest) -> Result<TranslationResponse, TranslationError> {
                self.calls += 1;
                if self.calls > 2 {
                    panic!("primary called while the circuit was open");
                }
                Err(TranslationError::Network("down".to_owned()))
            }

            fn is_available(&self, _s: Language, _t: Language) -> bool {
                true
            }

            fn engine_kind(&self) -> EngineKind {
                EngineKind::Online
            }
        }

        let breaker = CircuitBreaker::new(2, 60_000);
        let mut fallback = FallbackEngine::new(PanicIfCalledAgain { calls: 0 }, StubTranslationEngine::new(), breaker);
        let request = TranslationRequest {
            items: vec![TranslateItem {
                id: 1,
                text: "Quit".to_owned(),
                kind: None,
            }],
            source_language: Language::English,
            target_language: Language::Russian,
            context: None,
            app_id: None,
        };

        for _ in 0..3 {
            let translated = fallback.translate(&request).unwrap();
            assert_eq!(translated.items[0].translated, "Выход");
        }
        assert_eq!(fallback.circuit_breaker().state(), CircuitState::Open);
    }
}
