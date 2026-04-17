use std::collections::VecDeque;

const DEFAULT_REQUESTS_PER_TICK: u32 = 6;
const DEFAULT_SERVICE_CAPACITY_PER_TICK: u32 = 2;
const DEFAULT_MAX_QUEUE_SIZE: usize = 24;
const DEFAULT_OVERLOAD_THRESHOLD_TICKS: u32 = 4;
const DEFAULT_FAILURE_THRESHOLD_TICKS: u32 = 10;
const DEFAULT_MAX_REQUEST_RETRIES: u8 = 2;
const DEFAULT_BACKPRESSURE_THRESHOLD_TICKS: u32 = 2;
const DEFAULT_OVERLOAD_RECOVERY_TICKS: u32 = 3;
const DEFAULT_HISTORY_LIMIT: usize = 30;

const QUEUE_PRESSURE_NUMERATOR: usize = 3;
const QUEUE_PRESSURE_DENOMINATOR: usize = 4;
const BACKPRESSURE_RATE_DIVISOR: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimulationConfig {
    pub requests_per_tick: u32,
    pub service_capacity_per_tick: u32,
    pub max_queue_size: usize,
    pub overload_threshold_ticks: u32,
    pub failure_threshold_ticks: u32,
    pub max_request_retries: u8,
    pub backpressure_threshold_ticks: u32,
    pub overload_recovery_ticks: u32,
    pub history_limit: usize,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            requests_per_tick: DEFAULT_REQUESTS_PER_TICK,
            service_capacity_per_tick: DEFAULT_SERVICE_CAPACITY_PER_TICK,
            max_queue_size: DEFAULT_MAX_QUEUE_SIZE,
            overload_threshold_ticks: DEFAULT_OVERLOAD_THRESHOLD_TICKS,
            failure_threshold_ticks: DEFAULT_FAILURE_THRESHOLD_TICKS,
            max_request_retries: DEFAULT_MAX_REQUEST_RETRIES,
            backpressure_threshold_ticks: DEFAULT_BACKPRESSURE_THRESHOLD_TICKS,
            overload_recovery_ticks: DEFAULT_OVERLOAD_RECOVERY_TICKS,
            history_limit: DEFAULT_HISTORY_LIMIT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceId {
    A,
    B,
}

impl ServiceId {
    pub fn label(self) -> &'static str {
        match self {
            Self::A => "Service A",
            Self::B => "Service B",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    Healthy,
    Overloaded,
    Failed,
}

impl ServiceState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Overloaded => "overloaded",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueTrend {
    Rising,
    Falling,
    Stable,
}

impl QueueTrend {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rising => "rising",
            Self::Falling => "falling",
            Self::Stable => "stable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusSignal {
    QueueRising,
    SustainedBackpressure,
    RetryActivity,
    BothServicesUnhealthy,
}

#[derive(Debug, Clone, Copy)]
pub struct HistoryEntry {
    pub tick: u64,
    pub generated: u32,
    pub accepted: u32,
    pub processed: u32,
    pub dropped: u32,
    pub retried: u32,
    pub retry_exhausted: u32,
    pub failed_in_service: u32,
    pub queue_depth: usize,
    pub backpressure_active: bool,
    pub service_a_state: ServiceState,
    pub service_b_state: ServiceState,
}

#[derive(Debug, Clone, Copy)]
pub struct RecentSummary {
    pub window: usize,
    pub avg_generated: f64,
    pub avg_processed: f64,
    pub avg_dropped: f64,
    pub avg_retried: f64,
    pub backpressure_ticks: usize,
    pub queue_trend: QueueTrend,
}

#[derive(Debug, Clone, Copy)]
struct Request {
    retries: u8,
}

impl Request {
    fn new() -> Self {
        Self { retries: 0 }
    }
}

#[derive(Debug, Default)]
struct ServiceTickResult {
    retried: u32,
    retry_exhausted: u32,
    failed_in_service: u32,
}

#[derive(Debug)]
pub struct ServiceNode {
    pub id: ServiceId,
    pub capacity_per_tick: u32,
    pub state: ServiceState,
    pub processed: u64,
    pub failed_in_service: u64,
    pub retry_attempts: u64,
    pub retry_exhausted: u64,
    pub last_processed: u32,
    pub last_failed_in_service: u32,
    pub pressure_ticks: u32,
    pub recovery_ticks: u32,
}

impl ServiceNode {
    fn new(id: ServiceId, capacity_per_tick: u32) -> Self {
        Self {
            id,
            capacity_per_tick,
            state: ServiceState::Healthy,
            processed: 0,
            failed_in_service: 0,
            retry_attempts: 0,
            retry_exhausted: 0,
            last_processed: 0,
            last_failed_in_service: 0,
            pressure_ticks: 0,
            recovery_ticks: 0,
        }
    }

    pub fn label(&self) -> &'static str {
        self.id.label()
    }

    fn reset_tick(&mut self) {
        self.last_processed = 0;
        self.last_failed_in_service = 0;
    }

    fn process_from_queue(
        &mut self,
        queue: &mut VecDeque<Request>,
        retry_buffer: &mut VecDeque<Request>,
        max_request_retries: u8,
    ) -> ServiceTickResult {
        if self.state == ServiceState::Failed {
            return self.retry_offered_work(queue, retry_buffer, max_request_retries);
        }

        for _ in 0..self.capacity_per_tick {
            let Some(_request) = queue.pop_front() else {
                break;
            };

            self.processed += 1;
            self.last_processed += 1;
        }

        ServiceTickResult::default()
    }

    fn retry_offered_work(
        &mut self,
        queue: &mut VecDeque<Request>,
        retry_buffer: &mut VecDeque<Request>,
        max_request_retries: u8,
    ) -> ServiceTickResult {
        let mut result = ServiceTickResult::default();

        for _ in 0..self.capacity_per_tick {
            let Some(mut request) = queue.pop_front() else {
                break;
            };

            self.failed_in_service += 1;
            self.last_failed_in_service += 1;
            result.failed_in_service += 1;

            if request.retries < max_request_retries {
                request.retries += 1;
                retry_buffer.push_back(request);
                self.retry_attempts += 1;
                result.retried += 1;
            } else {
                self.retry_exhausted += 1;
                result.retry_exhausted += 1;
            }
        }

        result
    }

    fn update_state(
        &mut self,
        config: SimulationConfig,
        queue_pressure: bool,
        dropping_requests: bool,
    ) {
        if self.state == ServiceState::Failed {
            return;
        }

        if queue_pressure || dropping_requests {
            self.pressure_ticks += 1;
            self.recovery_ticks = 0;

            self.state = if self.pressure_ticks >= config.failure_threshold_ticks {
                ServiceState::Failed
            } else if self.pressure_ticks >= config.overload_threshold_ticks {
                ServiceState::Overloaded
            } else {
                ServiceState::Healthy
            };

            return;
        }

        match self.state {
            ServiceState::Healthy => {
                self.pressure_ticks = 0;
                self.recovery_ticks = 0;
            }
            ServiceState::Overloaded => {
                self.recovery_ticks += 1;

                if self.recovery_ticks >= config.overload_recovery_ticks {
                    self.state = ServiceState::Healthy;
                    self.pressure_ticks = 0;
                    self.recovery_ticks = 0;
                }
            }
            ServiceState::Failed => {}
        }
    }

    fn restart(&mut self) {
        if self.state == ServiceState::Failed {
            self.state = ServiceState::Healthy;
            self.pressure_ticks = 0;
            self.recovery_ticks = 0;
        }
    }
}

#[derive(Debug)]
pub struct Simulation {
    pub tick: u64,
    pub generated: u64,
    pub accepted: u64,
    pub dropped: u64,
    pub retry_attempts: u64,
    pub retry_exhausted: u64,
    pub last_generated: u32,
    pub last_accepted: u32,
    pub last_dropped: u32,
    pub last_retried: u32,
    pub last_retry_exhausted: u32,
    pub last_failed_in_service: u32,
    pub backpressure_active: bool,
    pub backpressure_ticks: u32,
    pub config: SimulationConfig,
    pub service_a: ServiceNode,
    pub service_b: ServiceNode,
    queue: VecDeque<Request>,
    history: VecDeque<HistoryEntry>,
}

impl Simulation {
    pub fn new(config: SimulationConfig) -> Self {
        Self {
            tick: 0,
            generated: 0,
            accepted: 0,
            dropped: 0,
            retry_attempts: 0,
            retry_exhausted: 0,
            last_generated: 0,
            last_accepted: 0,
            last_dropped: 0,
            last_retried: 0,
            last_retry_exhausted: 0,
            last_failed_in_service: 0,
            backpressure_active: false,
            backpressure_ticks: 0,
            config,
            service_a: ServiceNode::new(ServiceId::A, config.service_capacity_per_tick),
            service_b: ServiceNode::new(ServiceId::B, config.service_capacity_per_tick),
            queue: VecDeque::with_capacity(config.max_queue_size),
            history: VecDeque::with_capacity(config.history_limit),
        }
    }

    pub fn tick(&mut self) {
        self.tick += 1;
        self.last_generated = 0;
        self.last_accepted = 0;
        self.last_dropped = 0;
        self.last_retried = 0;
        self.last_retry_exhausted = 0;
        self.last_failed_in_service = 0;
        self.service_a.reset_tick();
        self.service_b.reset_tick();

        self.update_backpressure();
        self.generate_requests();
        self.dispatch_requests();
        self.update_service_states();
        self.record_history();
    }

    pub fn queue_depth(&self) -> usize {
        self.queue.len()
    }

    pub fn total_processed(&self) -> u64 {
        self.service_a.processed + self.service_b.processed
    }

    pub fn total_failed_in_service(&self) -> u64 {
        self.service_a.failed_in_service + self.service_b.failed_in_service
    }

    pub fn history(&self) -> &VecDeque<HistoryEntry> {
        &self.history
    }

    pub fn recent_summary(&self) -> RecentSummary {
        summarize_history(&self.history)
    }

    pub fn status_signals(&self) -> Vec<StatusSignal> {
        derive_status_signals(&self.history)
    }

    pub fn service(&self, id: ServiceId) -> &ServiceNode {
        match id {
            ServiceId::A => &self.service_a,
            ServiceId::B => &self.service_b,
        }
    }

    pub fn service_mut(&mut self, id: ServiceId) -> &mut ServiceNode {
        match id {
            ServiceId::A => &mut self.service_a,
            ServiceId::B => &mut self.service_b,
        }
    }

    pub fn restart_service(&mut self, id: ServiceId) {
        self.service_mut(id).restart();
    }

    fn update_backpressure(&mut self) {
        if self.queue_len_at_pressure_level() {
            self.backpressure_ticks += 1;
        } else {
            self.backpressure_ticks = 0;
        }

        self.backpressure_active =
            self.backpressure_ticks >= self.config.backpressure_threshold_ticks;
    }

    fn current_generation_rate(&self) -> u32 {
        if !self.backpressure_active || self.config.requests_per_tick == 0 {
            return self.config.requests_per_tick;
        }

        (self.config.requests_per_tick / BACKPRESSURE_RATE_DIVISOR).max(1)
    }

    fn generate_requests(&mut self) {
        let requests_to_generate = self.current_generation_rate();
        self.last_generated = requests_to_generate;

        for _ in 0..requests_to_generate {
            self.generated += 1;

            if self.queue.len() < self.config.max_queue_size {
                self.queue.push_back(Request::new());
                self.accepted += 1;
                self.last_accepted += 1;
            } else {
                self.dropped += 1;
                self.last_dropped += 1;
            }
        }
    }

    fn dispatch_requests(&mut self) {
        let mut retry_buffer = VecDeque::new();

        let service_a_result = self.service_a.process_from_queue(
            &mut self.queue,
            &mut retry_buffer,
            self.config.max_request_retries,
        );
        let service_b_result = self.service_b.process_from_queue(
            &mut self.queue,
            &mut retry_buffer,
            self.config.max_request_retries,
        );

        self.last_retried = service_a_result.retried + service_b_result.retried;
        self.last_retry_exhausted =
            service_a_result.retry_exhausted + service_b_result.retry_exhausted;
        self.last_failed_in_service =
            service_a_result.failed_in_service + service_b_result.failed_in_service;
        self.retry_attempts += self.last_retried as u64;
        self.retry_exhausted += self.last_retry_exhausted as u64;
        self.queue.extend(retry_buffer);
    }

    fn update_service_states(&mut self) {
        let queue_pressure = self.queue_len_at_pressure_level();
        let dropping_requests = self.last_dropped > 0;

        self.service_a
            .update_state(self.config, queue_pressure, dropping_requests);
        self.service_b
            .update_state(self.config, queue_pressure, dropping_requests);
    }

    fn queue_len_at_pressure_level(&self) -> bool {
        self.queue.len().saturating_mul(QUEUE_PRESSURE_DENOMINATOR)
            >= self
                .config
                .max_queue_size
                .saturating_mul(QUEUE_PRESSURE_NUMERATOR)
    }

    fn record_history(&mut self) {
        if self.config.history_limit == 0 {
            return;
        }

        if self.history.len() == self.config.history_limit {
            self.history.pop_front();
        }

        self.history.push_back(HistoryEntry {
            tick: self.tick,
            generated: self.last_generated,
            accepted: self.last_accepted,
            processed: self.service_a.last_processed + self.service_b.last_processed,
            dropped: self.last_dropped,
            retried: self.last_retried,
            retry_exhausted: self.last_retry_exhausted,
            failed_in_service: self.last_failed_in_service,
            queue_depth: self.queue_depth(),
            backpressure_active: self.backpressure_active,
            service_a_state: self.service_a.state,
            service_b_state: self.service_b.state,
        });
    }
}

fn summarize_history(history: &VecDeque<HistoryEntry>) -> RecentSummary {
    let window = history.len();

    if window == 0 {
        return RecentSummary {
            window: 0,
            avg_generated: 0.0,
            avg_processed: 0.0,
            avg_dropped: 0.0,
            avg_retried: 0.0,
            backpressure_ticks: 0,
            queue_trend: QueueTrend::Stable,
        };
    }

    let generated: u32 = history.iter().map(|entry| entry.generated).sum();
    let processed: u32 = history.iter().map(|entry| entry.processed).sum();
    let dropped: u32 = history.iter().map(|entry| entry.dropped).sum();
    let retried: u32 = history.iter().map(|entry| entry.retried).sum();
    let backpressure_ticks = history
        .iter()
        .filter(|entry| entry.backpressure_active)
        .count();
    let window_f64 = window as f64;

    RecentSummary {
        window,
        avg_generated: generated as f64 / window_f64,
        avg_processed: processed as f64 / window_f64,
        avg_dropped: dropped as f64 / window_f64,
        avg_retried: retried as f64 / window_f64,
        backpressure_ticks,
        queue_trend: classify_queue_trend(history),
    }
}

fn classify_queue_trend(history: &VecDeque<HistoryEntry>) -> QueueTrend {
    let Some(first) = history.front() else {
        return QueueTrend::Stable;
    };
    let Some(last) = history.back() else {
        return QueueTrend::Stable;
    };

    if last.queue_depth > first.queue_depth.saturating_add(1) {
        QueueTrend::Rising
    } else if first.queue_depth > last.queue_depth.saturating_add(1) {
        QueueTrend::Falling
    } else {
        QueueTrend::Stable
    }
}

fn derive_status_signals(history: &VecDeque<HistoryEntry>) -> Vec<StatusSignal> {
    let summary = summarize_history(history);
    let mut signals = Vec::new();

    if summary.queue_trend == QueueTrend::Rising {
        signals.push(StatusSignal::QueueRising);
    }

    if summary.backpressure_ticks >= 3 {
        signals.push(StatusSignal::SustainedBackpressure);
    }

    if summary.avg_retried >= 1.0 {
        signals.push(StatusSignal::RetryActivity);
    }

    if let Some(latest) = history.back() {
        let service_a_unhealthy = latest.service_a_state != ServiceState::Healthy;
        let service_b_unhealthy = latest.service_b_state != ServiceState::Healthy;

        if service_a_unhealthy && service_b_unhealthy {
            signals.push(StatusSignal::BothServicesUnhealthy);
        }
    }

    signals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(
        requests_per_tick: u32,
        service_capacity_per_tick: u32,
        max_queue_size: usize,
    ) -> SimulationConfig {
        SimulationConfig {
            requests_per_tick,
            service_capacity_per_tick,
            max_queue_size,
            ..SimulationConfig::default()
        }
    }

    fn history_entry(
        tick: u64,
        generated: u32,
        processed: u32,
        dropped: u32,
        retried: u32,
        queue_depth: usize,
        backpressure_active: bool,
    ) -> HistoryEntry {
        HistoryEntry {
            tick,
            generated,
            accepted: generated,
            processed,
            dropped,
            retried,
            retry_exhausted: 0,
            failed_in_service: 0,
            queue_depth,
            backpressure_active,
            service_a_state: ServiceState::Healthy,
            service_b_state: ServiceState::Healthy,
        }
    }

    #[test]
    fn request_flow_is_deterministic_before_overload() {
        let config = SimulationConfig {
            overload_threshold_ticks: 4,
            failure_threshold_ticks: 8,
            ..test_config(3, 1, 10)
        };
        let mut simulation = Simulation::new(config);

        simulation.tick();
        simulation.tick();
        simulation.tick();

        assert_eq!(simulation.tick, 3);
        assert_eq!(simulation.generated, 9);
        assert_eq!(simulation.accepted, 9);
        assert_eq!(simulation.total_processed(), 6);
        assert_eq!(simulation.dropped, 0);
        assert_eq!(simulation.queue_depth(), 3);
        assert_eq!(simulation.service_a.state, ServiceState::Healthy);
        assert_eq!(simulation.service_b.state, ServiceState::Healthy);
    }

    #[test]
    fn dispatch_order_is_service_a_then_service_b() {
        let config = SimulationConfig {
            overload_threshold_ticks: 10,
            failure_threshold_ticks: 20,
            ..test_config(5, 2, 20)
        };
        let mut simulation = Simulation::new(config);

        simulation.tick();

        assert_eq!(simulation.service_a.last_processed, 2);
        assert_eq!(simulation.service_b.last_processed, 2);
        assert_eq!(simulation.queue_depth(), 1);

        simulation.tick();

        assert_eq!(simulation.service_a.processed, 4);
        assert_eq!(simulation.service_b.processed, 4);
        assert_eq!(simulation.queue_depth(), 2);
    }

    #[test]
    fn both_services_participate_in_processing() {
        let config = SimulationConfig {
            overload_threshold_ticks: 10,
            failure_threshold_ticks: 20,
            ..test_config(4, 2, 10)
        };
        let mut simulation = Simulation::new(config);

        simulation.tick();

        assert_eq!(simulation.service_a.last_processed, 2);
        assert_eq!(simulation.service_b.last_processed, 2);
        assert_eq!(simulation.total_processed(), 4);
        assert_eq!(simulation.queue_depth(), 0);
    }

    #[test]
    fn sustained_pressure_overloads_then_fails_services() {
        let config = SimulationConfig {
            overload_threshold_ticks: 2,
            failure_threshold_ticks: 4,
            backpressure_threshold_ticks: 20,
            ..test_config(6, 1, 8)
        };
        let mut simulation = Simulation::new(config);

        for _ in 0..3 {
            simulation.tick();
        }

        assert_eq!(simulation.service_a.state, ServiceState::Overloaded);
        assert_eq!(simulation.service_b.state, ServiceState::Overloaded);

        for _ in 0..2 {
            simulation.tick();
        }

        assert_eq!(simulation.service_a.state, ServiceState::Failed);
        assert_eq!(simulation.service_b.state, ServiceState::Failed);
    }

    #[test]
    fn queue_overflow_counts_dropped_requests() {
        let config = SimulationConfig {
            overload_threshold_ticks: 10,
            failure_threshold_ticks: 20,
            ..test_config(5, 0, 3)
        };
        let mut simulation = Simulation::new(config);

        simulation.tick();

        assert_eq!(simulation.generated, 5);
        assert_eq!(simulation.accepted, 3);
        assert_eq!(simulation.dropped, 2);
        assert_eq!(simulation.last_dropped, 2);
        assert_eq!(simulation.queue_depth(), 3);
        assert_eq!(simulation.total_processed(), 0);
    }

    #[test]
    fn one_failed_service_does_not_stop_the_other_service() {
        let config = SimulationConfig {
            overload_threshold_ticks: 10,
            failure_threshold_ticks: 20,
            ..test_config(5, 2, 10)
        };
        let mut simulation = Simulation::new(config);
        simulation.service_a.state = ServiceState::Failed;

        simulation.tick();

        assert_eq!(simulation.service_a.processed, 0);
        assert_eq!(simulation.service_a.failed_in_service, 2);
        assert_eq!(simulation.service_a.retry_attempts, 2);
        assert_eq!(simulation.service_b.last_processed, 2);
        assert_eq!(simulation.service_b.processed, 2);
        assert_eq!(simulation.queue_depth(), 3);
    }

    #[test]
    fn retries_are_bounded() {
        let config = SimulationConfig {
            max_request_retries: 1,
            overload_threshold_ticks: 10,
            failure_threshold_ticks: 20,
            ..test_config(2, 1, 10)
        };
        let mut simulation = Simulation::new(config);
        simulation.service_a.state = ServiceState::Failed;
        simulation.service_b.state = ServiceState::Failed;

        simulation.tick();

        assert_eq!(simulation.last_retried, 2);
        assert_eq!(simulation.retry_attempts, 2);
        assert_eq!(simulation.retry_exhausted, 0);
        assert_eq!(simulation.queue_depth(), 2);

        simulation.tick();

        assert_eq!(simulation.last_retry_exhausted, 2);
        assert_eq!(simulation.retry_exhausted, 2);
        assert_eq!(simulation.queue_depth(), 2);
    }

    #[test]
    fn retry_logic_is_deterministic() {
        let config = SimulationConfig {
            max_request_retries: 1,
            overload_threshold_ticks: 10,
            failure_threshold_ticks: 20,
            ..test_config(3, 1, 10)
        };
        let mut first = Simulation::new(config);
        let mut second = Simulation::new(config);
        first.service_a.state = ServiceState::Failed;
        first.service_b.state = ServiceState::Failed;
        second.service_a.state = ServiceState::Failed;
        second.service_b.state = ServiceState::Failed;

        for _ in 0..4 {
            first.tick();
            second.tick();
        }

        assert_eq!(first.generated, second.generated);
        assert_eq!(first.queue_depth(), second.queue_depth());
        assert_eq!(first.retry_attempts, second.retry_attempts);
        assert_eq!(first.retry_exhausted, second.retry_exhausted);
        assert_eq!(
            first.total_failed_in_service(),
            second.total_failed_in_service()
        );
    }

    #[test]
    fn backpressure_activates_under_sustained_queue_pressure() {
        let config = SimulationConfig {
            backpressure_threshold_ticks: 1,
            overload_threshold_ticks: 10,
            failure_threshold_ticks: 20,
            ..test_config(6, 0, 20)
        };
        let mut simulation = Simulation::new(config);

        while !simulation.backpressure_active {
            simulation.tick();
        }

        simulation.tick();

        assert!(simulation.backpressure_active);
        assert_eq!(simulation.last_generated, 3);
    }

    #[test]
    fn history_buffer_remains_bounded() {
        let config = SimulationConfig {
            history_limit: 3,
            ..test_config(2, 1, 10)
        };
        let mut simulation = Simulation::new(config);

        for _ in 0..5 {
            simulation.tick();
        }

        assert_eq!(simulation.history().len(), 3);
        assert_eq!(simulation.history().front().unwrap().tick, 3);
        assert_eq!(simulation.history().back().unwrap().tick, 5);
    }

    #[test]
    fn history_values_are_recorded_deterministically() {
        let config = SimulationConfig {
            overload_threshold_ticks: 10,
            failure_threshold_ticks: 20,
            ..test_config(5, 2, 20)
        };
        let mut simulation = Simulation::new(config);

        simulation.tick();
        simulation.tick();

        let first = simulation.history().front().unwrap();
        assert_eq!(first.tick, 1);
        assert_eq!(first.generated, 5);
        assert_eq!(first.accepted, 5);
        assert_eq!(first.processed, 4);
        assert_eq!(first.dropped, 0);
        assert_eq!(first.retried, 0);
        assert_eq!(first.retry_exhausted, 0);
        assert_eq!(first.failed_in_service, 0);
        assert_eq!(first.queue_depth, 1);
        assert!(!first.backpressure_active);

        let second = simulation.history().back().unwrap();
        assert_eq!(second.tick, 2);
        assert_eq!(second.generated, 5);
        assert_eq!(second.processed, 4);
        assert_eq!(second.queue_depth, 2);
    }

    #[test]
    fn recent_summary_calculations_are_correct() {
        let mut history = VecDeque::new();
        history.push_back(history_entry(1, 4, 2, 0, 0, 1, false));
        history.push_back(history_entry(2, 2, 4, 1, 2, 3, true));
        history.push_back(history_entry(3, 6, 3, 2, 1, 5, true));

        let summary = summarize_history(&history);

        assert_eq!(summary.window, 3);
        assert!((summary.avg_generated - 4.0).abs() < f64::EPSILON);
        assert!((summary.avg_processed - 3.0).abs() < f64::EPSILON);
        assert!((summary.avg_dropped - 1.0).abs() < f64::EPSILON);
        assert!((summary.avg_retried - 1.0).abs() < f64::EPSILON);
        assert_eq!(summary.backpressure_ticks, 2);
        assert_eq!(summary.queue_trend, QueueTrend::Rising);
    }

    #[test]
    fn queue_trend_classification_handles_basic_shapes() {
        let mut rising = VecDeque::new();
        rising.push_back(history_entry(1, 0, 0, 0, 0, 1, false));
        rising.push_back(history_entry(2, 0, 0, 0, 0, 4, false));
        assert_eq!(classify_queue_trend(&rising), QueueTrend::Rising);

        let mut falling = VecDeque::new();
        falling.push_back(history_entry(1, 0, 0, 0, 0, 5, false));
        falling.push_back(history_entry(2, 0, 0, 0, 0, 2, false));
        assert_eq!(classify_queue_trend(&falling), QueueTrend::Falling);

        let mut stable = VecDeque::new();
        stable.push_back(history_entry(1, 0, 0, 0, 0, 5, false));
        stable.push_back(history_entry(2, 0, 0, 0, 0, 6, false));
        assert_eq!(classify_queue_trend(&stable), QueueTrend::Stable);
    }

    #[test]
    fn status_signal_derivation_uses_recent_history() {
        let mut history = VecDeque::new();
        history.push_back(history_entry(1, 2, 1, 0, 1, 1, true));
        history.push_back(history_entry(2, 2, 1, 0, 1, 3, true));
        let mut latest = history_entry(3, 2, 1, 0, 1, 6, true);
        latest.service_a_state = ServiceState::Overloaded;
        latest.service_b_state = ServiceState::Failed;
        history.push_back(latest);

        let signals = derive_status_signals(&history);

        assert!(signals.contains(&StatusSignal::QueueRising));
        assert!(signals.contains(&StatusSignal::SustainedBackpressure));
        assert!(signals.contains(&StatusSignal::RetryActivity));
        assert!(signals.contains(&StatusSignal::BothServicesUnhealthy));
    }

    #[test]
    fn overloaded_service_recovers_when_pressure_stays_low() {
        let config = SimulationConfig {
            requests_per_tick: 0,
            service_capacity_per_tick: 0,
            max_queue_size: 4,
            overload_threshold_ticks: 1,
            failure_threshold_ticks: 10,
            overload_recovery_ticks: 2,
            ..SimulationConfig::default()
        };
        let mut simulation = Simulation::new(config);
        simulation
            .queue
            .extend([Request::new(), Request::new(), Request::new()]);

        simulation.tick();
        assert_eq!(simulation.service_a.state, ServiceState::Overloaded);
        assert_eq!(simulation.service_b.state, ServiceState::Overloaded);

        simulation.queue.clear();
        simulation.tick();
        assert_eq!(simulation.service_a.state, ServiceState::Overloaded);
        assert_eq!(simulation.service_a.recovery_ticks, 1);

        simulation.tick();
        assert_eq!(simulation.service_a.state, ServiceState::Healthy);
        assert_eq!(simulation.service_b.state, ServiceState::Healthy);
        assert_eq!(simulation.service_a.pressure_ticks, 0);
    }

    #[test]
    fn failed_service_still_requires_manual_restart() {
        let config = SimulationConfig {
            requests_per_tick: 0,
            service_capacity_per_tick: 0,
            max_queue_size: 4,
            overload_threshold_ticks: 1,
            failure_threshold_ticks: 2,
            overload_recovery_ticks: 1,
            ..SimulationConfig::default()
        };
        let mut simulation = Simulation::new(config);
        simulation
            .queue
            .extend([Request::new(), Request::new(), Request::new()]);

        simulation.tick();
        simulation.tick();
        assert_eq!(simulation.service_a.state, ServiceState::Failed);

        simulation.queue.clear();
        simulation.tick();
        simulation.tick();
        assert_eq!(simulation.service_a.state, ServiceState::Failed);

        simulation.restart_service(ServiceId::A);
        assert_eq!(simulation.service_a.state, ServiceState::Healthy);
    }

    #[test]
    fn restart_failed_service_resets_state_and_pressure_only() {
        let config = SimulationConfig {
            overload_threshold_ticks: 2,
            failure_threshold_ticks: 4,
            backpressure_threshold_ticks: 20,
            ..test_config(6, 1, 8)
        };
        let mut simulation = Simulation::new(config);

        while simulation.service_a.state != ServiceState::Failed {
            simulation.tick();
        }

        let generated = simulation.generated;
        let processed_a = simulation.service_a.processed;
        let failed_a = simulation.service_a.failed_in_service;
        let retries_a = simulation.service_a.retry_attempts;
        let processed_b = simulation.service_b.processed;
        let failed_b = simulation.service_b.failed_in_service;
        let dropped = simulation.dropped;

        simulation.restart_service(ServiceId::A);

        assert_eq!(simulation.service_a.state, ServiceState::Healthy);
        assert_eq!(simulation.service_a.pressure_ticks, 0);
        assert_eq!(simulation.service_a.recovery_ticks, 0);
        assert_eq!(simulation.generated, generated);
        assert_eq!(simulation.service_a.processed, processed_a);
        assert_eq!(simulation.service_a.failed_in_service, failed_a);
        assert_eq!(simulation.service_a.retry_attempts, retries_a);
        assert_eq!(simulation.service_b.processed, processed_b);
        assert_eq!(simulation.service_b.failed_in_service, failed_b);
        assert_eq!(simulation.dropped, dropped);
    }
}
