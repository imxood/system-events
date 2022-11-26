use system_events::{Monitor, SystemMonitor};

fn main() {
    let monitor = SystemMonitor::new();

    while let Some(event) = monitor.recv(None) {
        println!("{:?}", &event);
    }
}
