use system_events::{Monitor, SystemMonitor};

fn main() {
    let notify = system_events::EventNotify::new(Box::new(move |_| {
        println!("new event");
    }));

    let monitor = SystemMonitor::new(notify);

    while let Some(event) = monitor.recv(None) {
        println!("event: {event:?}");
    }
}
