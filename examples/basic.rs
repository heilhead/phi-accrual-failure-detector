use {
    phi_accrual_failure_detector::{Detector as _, UnsyncDetector},
    std::{thread, time::Duration},
};

fn main() {
    let detector = UnsyncDetector::builder().build().unwrap();

    println!("heartbeat");
    detector.heartbeat();
    thread::sleep(Duration::from_millis(1000));

    println!("heartbeat");
    detector.heartbeat();
    thread::sleep(Duration::from_millis(1000));

    println!("heartbeat");
    detector.heartbeat();
    thread::sleep(Duration::from_millis(1000));

    // The resource is available when receiving regular heartbeats.
    println!("is available: {}", detector.is_available());
    assert!(detector.is_available());

    thread::sleep(Duration::from_millis(4000));

    // The resource is no longer available, since it's missed a few heartbeats.
    println!("is available: {}", detector.is_available());
    assert!(!detector.is_available());
}
