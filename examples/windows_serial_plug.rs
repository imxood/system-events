use system_events::{Monitor, SystemMonitor};

fn main() {
    let monitor = SystemMonitor::default();

    while let Some(event) = monitor.recv(None) {
        println!("event: {event:?}");
    }
}
