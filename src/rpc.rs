use std::{
    io::{self, BufRead, BufReader, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::{
    scenario::Scenario,
    simulation::{QueueTrend, ServiceNode, ServiceState, Simulation, StatusSignal},
};

const GET_SIMULATION_SNAPSHOT: &str = "GetSimulationSnapshot";
const PING: &str = "Ping";
const ACCEPT_SLEEP: Duration = Duration::from_millis(20);
const CLIENT_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq)]
pub struct SimulationSnapshot {
    pub scenario: String,
    pub tick: u64,
    pub queue: QueueSnapshot,
    pub backpressure_active: bool,
    pub totals: TotalsSnapshot,
    pub recent: RecentSnapshot,
    pub service_a: ServiceSnapshot,
    pub service_b: ServiceSnapshot,
    pub status_signals: Vec<String>,
}

impl SimulationSnapshot {
    pub fn from_simulation(scenario: Scenario, simulation: &Simulation) -> Self {
        let recent = simulation.recent_summary();

        Self {
            scenario: scenario.name().to_string(),
            tick: simulation.tick,
            queue: QueueSnapshot {
                depth: simulation.queue_depth(),
                max: simulation.config.max_queue_size,
            },
            backpressure_active: simulation.backpressure_active,
            totals: TotalsSnapshot {
                generated: simulation.generated,
                accepted: simulation.accepted,
                processed: simulation.total_processed(),
                dropped: simulation.dropped,
                retried: simulation.retry_attempts,
                retry_exhausted: simulation.retry_exhausted,
                failed_in_service: simulation.total_failed_in_service(),
            },
            recent: RecentSnapshot {
                window: recent.window,
                avg_generated: recent.avg_generated,
                avg_processed: recent.avg_processed,
                avg_dropped: recent.avg_dropped,
                avg_retried: recent.avg_retried,
                backpressure_ticks: recent.backpressure_ticks,
                queue_trend: queue_trend_name(recent.queue_trend).to_string(),
            },
            service_a: ServiceSnapshot::from_service(&simulation.service_a),
            service_b: ServiceSnapshot::from_service(&simulation.service_b),
            status_signals: simulation
                .status_signals()
                .into_iter()
                .map(|signal| status_signal_name(signal).to_string())
                .collect(),
        }
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\"scenario\":{},\"tick\":{},\"queue\":{},\"backpressure_active\":{},\"totals\":{},\"recent\":{},\"service_a\":{},\"service_b\":{},\"status_signals\":{}}}",
            json_string(&self.scenario),
            self.tick,
            self.queue.to_json(),
            self.backpressure_active,
            self.totals.to_json(),
            self.recent.to_json(),
            self.service_a.to_json(),
            self.service_b.to_json(),
            json_string_array(&self.status_signals)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueSnapshot {
    pub depth: usize,
    pub max: usize,
}

impl QueueSnapshot {
    fn to_json(&self) -> String {
        format!("{{\"depth\":{},\"max\":{}}}", self.depth, self.max)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TotalsSnapshot {
    pub generated: u64,
    pub accepted: u64,
    pub processed: u64,
    pub dropped: u64,
    pub retried: u64,
    pub retry_exhausted: u64,
    pub failed_in_service: u64,
}

impl TotalsSnapshot {
    fn to_json(&self) -> String {
        format!(
            "{{\"generated\":{},\"accepted\":{},\"processed\":{},\"dropped\":{},\"retried\":{},\"retry_exhausted\":{},\"failed_in_service\":{}}}",
            self.generated,
            self.accepted,
            self.processed,
            self.dropped,
            self.retried,
            self.retry_exhausted,
            self.failed_in_service
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecentSnapshot {
    pub window: usize,
    pub avg_generated: f64,
    pub avg_processed: f64,
    pub avg_dropped: f64,
    pub avg_retried: f64,
    pub backpressure_ticks: usize,
    pub queue_trend: String,
}

impl RecentSnapshot {
    fn to_json(&self) -> String {
        format!(
            "{{\"window\":{},\"avg_generated\":{:.3},\"avg_processed\":{:.3},\"avg_dropped\":{:.3},\"avg_retried\":{:.3},\"backpressure_ticks\":{},\"queue_trend\":{}}}",
            self.window,
            self.avg_generated,
            self.avg_processed,
            self.avg_dropped,
            self.avg_retried,
            self.backpressure_ticks,
            json_string(&self.queue_trend)
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceSnapshot {
    pub name: String,
    pub state: String,
    pub capacity_per_tick: u32,
    pub processed: u64,
    pub failed_in_service: u64,
    pub retry_attempts: u64,
    pub retry_exhausted: u64,
    pub last_processed: u32,
    pub last_failed_in_service: u32,
    pub pressure_ticks: u32,
    pub recovery_ticks: u32,
}

impl ServiceSnapshot {
    fn from_service(service: &ServiceNode) -> Self {
        Self {
            name: service.label().to_string(),
            state: service_state_name(service.state).to_string(),
            capacity_per_tick: service.capacity_per_tick,
            processed: service.processed,
            failed_in_service: service.failed_in_service,
            retry_attempts: service.retry_attempts,
            retry_exhausted: service.retry_exhausted,
            last_processed: service.last_processed,
            last_failed_in_service: service.last_failed_in_service,
            pressure_ticks: service.pressure_ticks,
            recovery_ticks: service.recovery_ticks,
        }
    }

    fn to_json(&self) -> String {
        format!(
            "{{\"name\":{},\"state\":{},\"capacity_per_tick\":{},\"processed\":{},\"failed_in_service\":{},\"retry_attempts\":{},\"retry_exhausted\":{},\"last_processed\":{},\"last_failed_in_service\":{},\"pressure_ticks\":{},\"recovery_ticks\":{}}}",
            json_string(&self.name),
            json_string(&self.state),
            self.capacity_per_tick,
            self.processed,
            self.failed_in_service,
            self.retry_attempts,
            self.retry_exhausted,
            self.last_processed,
            self.last_failed_in_service,
            self.pressure_ticks,
            self.recovery_ticks
        )
    }
}

#[derive(Clone)]
pub struct SnapshotStore {
    scenario: Scenario,
    snapshot: Arc<Mutex<SimulationSnapshot>>,
}

impl SnapshotStore {
    pub fn new(scenario: Scenario, simulation: &Simulation) -> Self {
        Self {
            scenario,
            snapshot: Arc::new(Mutex::new(SimulationSnapshot::from_simulation(
                scenario, simulation,
            ))),
        }
    }

    pub fn update(&self, simulation: &Simulation) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            *snapshot = SimulationSnapshot::from_simulation(self.scenario, simulation);
        }
    }

    fn snapshot(&self) -> io::Result<SimulationSnapshot> {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "snapshot store is unavailable"))
    }
}

pub struct RpcServer {
    addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl RpcServer {
    pub fn start(addr: &str, store: SnapshotStore) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;

        let addr = listener.local_addr()?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let handle = thread::spawn(move || run_server(listener, store, server_shutdown));

        Ok(Self {
            addr,
            shutdown,
            handle: Some(handle),
        })
    }

    #[cfg(test)]
    fn local_addr(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for RpcServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);

        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn run_server(listener: TcpListener, store: SnapshotStore, shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = handle_client(stream, &store);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_SLEEP);
            }
            Err(_) => break,
        }
    }
}

fn handle_client(mut stream: TcpStream, store: &SnapshotStore) -> io::Result<()> {
    stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
    stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;

    let mut request = String::new();
    let mut reader = BufReader::new(stream.try_clone()?);
    reader.read_line(&mut request)?;

    let response = handle_request(request.trim(), store);
    stream.write_all(response.as_bytes())?;
    stream.write_all(b"\n")?;

    Ok(())
}

fn handle_request(request: &str, store: &SnapshotStore) -> String {
    match parse_method(request).as_deref() {
        Some(GET_SIMULATION_SNAPSHOT) => match store.snapshot() {
            Ok(snapshot) => ok_snapshot_response(&snapshot),
            Err(error) => error_response(&error.to_string()),
        },
        Some(PING) => "{\"ok\":true,\"message\":\"pong\"}".to_string(),
        Some(_) => error_response("unknown method"),
        None => error_response("invalid request"),
    }
}

fn parse_method(request: &str) -> Option<String> {
    let key_index = request.find("\"method\"")?;
    let rest = &request[key_index + "\"method\"".len()..];
    let colon_index = rest.find(':')?;
    let value = rest[colon_index + 1..].trim_start();
    let value = value.strip_prefix('"')?;
    let end_index = value.find('"')?;

    Some(value[..end_index].to_string())
}

fn ok_snapshot_response(snapshot: &SimulationSnapshot) -> String {
    format!("{{\"ok\":true,\"snapshot\":{}}}", snapshot.to_json())
}

fn error_response(message: &str) -> String {
    format!("{{\"ok\":false,\"error\":{}}}", json_string(message))
}

fn service_state_name(state: ServiceState) -> &'static str {
    match state {
        ServiceState::Healthy => "healthy",
        ServiceState::Overloaded => "overloaded",
        ServiceState::Failed => "failed",
    }
}

fn queue_trend_name(trend: QueueTrend) -> &'static str {
    match trend {
        QueueTrend::Rising => "rising",
        QueueTrend::Falling => "falling",
        QueueTrend::Stable => "stable",
    }
}

fn status_signal_name(signal: StatusSignal) -> &'static str {
    match signal {
        StatusSignal::QueueRising => "queue_rising",
        StatusSignal::SustainedBackpressure => "sustained_backpressure",
        StatusSignal::RetryActivity => "retry_activity",
        StatusSignal::BothServicesUnhealthy => "both_services_unhealthy",
    }
}

fn json_string(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() + 2);
    encoded.push('"');

    for ch in value.chars() {
        match ch {
            '"' => encoded.push_str("\\\""),
            '\\' => encoded.push_str("\\\\"),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            _ => encoded.push(ch),
        }
    }

    encoded.push('"');
    encoded
}

fn json_string_array(values: &[String]) -> String {
    let encoded = values
        .iter()
        .map(|value| json_string(value))
        .collect::<Vec<_>>()
        .join(",");

    format!("[{encoded}]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulation::{SimulationConfig, StatusSignal};
    use std::io::Read;

    #[test]
    fn snapshot_maps_current_simulation_state() {
        let mut simulation = Simulation::new(SimulationConfig::default());

        simulation.tick();
        simulation.tick();

        let snapshot = SimulationSnapshot::from_simulation(Scenario::PressureRamp, &simulation);

        assert_eq!(snapshot.scenario, "pressure-ramp");
        assert_eq!(snapshot.tick, 2);
        assert_eq!(snapshot.queue.depth, 4);
        assert_eq!(snapshot.queue.max, simulation.config.max_queue_size);
        assert!(!snapshot.backpressure_active);
        assert_eq!(snapshot.totals.generated, 12);
        assert_eq!(snapshot.totals.accepted, 12);
        assert_eq!(snapshot.totals.processed, 8);
        assert_eq!(snapshot.totals.dropped, 0);
        assert_eq!(snapshot.service_a.state, "healthy");
        assert_eq!(snapshot.service_b.state, "healthy");
        assert_eq!(snapshot.recent.window, 2);
        assert_eq!(snapshot.recent.avg_generated, 6.0);
        assert_eq!(snapshot.recent.avg_processed, 4.0);
        assert_eq!(snapshot.recent.queue_trend, "rising");
        assert_eq!(snapshot.status_signals, vec!["queue_rising"]);
    }

    #[test]
    fn rpc_endpoint_returns_snapshot() {
        let mut simulation = Simulation::new(SimulationConfig::default());
        simulation.tick();
        let store = SnapshotStore::new(Scenario::PressureRamp, &simulation);
        let server = RpcServer::start("127.0.0.1:0", store).expect("start rpc server");

        let response = send_request(
            server.local_addr(),
            "{\"method\":\"GetSimulationSnapshot\"}\n",
        )
        .expect("rpc response");

        assert!(response.contains("\"ok\":true"));
        assert!(response.contains("\"scenario\":\"pressure-ramp\""));
        assert!(response.contains("\"tick\":1"));
        assert!(response.contains("\"queue\":{\"depth\":2,\"max\":24}"));
        assert!(response.contains("\"service_a\""));
        assert!(response.contains("\"service_b\""));
    }

    #[test]
    fn rpc_endpoint_rejects_unknown_methods_without_mutating_snapshot() {
        let mut simulation = Simulation::new(SimulationConfig::default());
        simulation.tick();
        let store = SnapshotStore::new(Scenario::PressureRamp, &simulation);
        let server = RpcServer::start("127.0.0.1:0", store).expect("start rpc server");

        let response = send_request(server.local_addr(), "{\"method\":\"RestartService\"}\n")
            .expect("rpc response");

        assert_eq!(
            response.trim(),
            "{\"ok\":false,\"error\":\"unknown method\"}"
        );
    }

    #[test]
    fn ping_returns_pong() {
        let simulation = Simulation::new(SimulationConfig::default());
        let store = SnapshotStore::new(Scenario::PressureRamp, &simulation);
        let server = RpcServer::start("127.0.0.1:0", store).expect("start rpc server");

        let response =
            send_request(server.local_addr(), "{\"method\":\"Ping\"}\n").expect("rpc response");

        assert_eq!(response.trim(), "{\"ok\":true,\"message\":\"pong\"}");
    }

    #[test]
    fn status_signal_names_are_stable_contract_values() {
        assert_eq!(
            status_signal_name(StatusSignal::QueueRising),
            "queue_rising"
        );
        assert_eq!(
            status_signal_name(StatusSignal::SustainedBackpressure),
            "sustained_backpressure"
        );
        assert_eq!(
            status_signal_name(StatusSignal::RetryActivity),
            "retry_activity"
        );
        assert_eq!(
            status_signal_name(StatusSignal::BothServicesUnhealthy),
            "both_services_unhealthy"
        );
    }

    fn send_request(addr: SocketAddr, request: &str) -> io::Result<String> {
        let mut stream = TcpStream::connect(addr)?;
        stream.write_all(request.as_bytes())?;
        stream.shutdown(std::net::Shutdown::Write)?;

        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        Ok(response)
    }
}
