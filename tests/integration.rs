use {
    phi_accrual_failure_detector::*,
    std::{
        sync::atomic::{AtomicU64, AtomicUsize, Ordering},
        thread,
        time::Duration,
    },
};

struct FakeClock {
    intervals: Vec<u64>,
    cursor: AtomicUsize,
    time: AtomicU64,
}

impl FakeClock {
    fn new(intervals: Vec<u64>) -> Self {
        assert!(!intervals.is_empty());

        Self {
            intervals,
            cursor: 1.into(),
            time: Default::default(),
        }
    }
}

impl Clock for FakeClock {
    type Timestamp = u64;

    fn timestamp(&self) -> Self::Timestamp {
        let idx = self.cursor.fetch_add(1, Ordering::Relaxed) % self.intervals.len();
        dbg!(self.time.fetch_add(self.intervals[idx], Ordering::Relaxed))
    }

    fn elapsed(before: &Self::Timestamp, after: &Self::Timestamp) -> Duration {
        Duration::from_millis(*after - *before)
    }
}

fn builder() -> Builder<UnsyncState<DefaultClock>> {
    FailureDetector::builder()
        .threshold(8.0)
        .max_sample_size(100)
        .min_std_deviation(Duration::from_millis(10))
        .acceptable_heartbeat_pause(Duration::ZERO)
        .first_heartbeat_estimate(Duration::from_secs(1))
}

#[test]
fn node_available() {
    let intervals = vec![0, 1000, 100, 100];
    let detector = builder().clock(FakeClock::new(intervals)).build().unwrap();
    detector.heartbeat();
    detector.heartbeat();
    detector.heartbeat();
    assert!(detector.is_available());
}

#[test]
fn node_heartbeat_missed_dead1() {
    let intervals = vec![0, 1000, 100, 100, 7000];
    let detector = builder().clock(FakeClock::new(intervals)).build().unwrap();

    detector.heartbeat(); // 0
    detector.heartbeat(); // 1000
    detector.heartbeat(); // 1100

    assert!(detector.is_available()); // 1200
    assert!(!detector.is_available()); // 8200
}

#[test]
fn node_heartbeat_missed_dead2() {
    let intervals = vec![0, 1000, 1000, 1000, 1000, 1000, 500, 500, 5000];
    let detector = builder()
        .acceptable_heartbeat_pause(Duration::from_secs(3))
        .clock(FakeClock::new(intervals))
        .build()
        .unwrap();

    detector.heartbeat(); // 0
    detector.heartbeat(); // 1000
    detector.heartbeat(); // 2000
    detector.heartbeat(); // 3000
    detector.heartbeat(); // 4000
    detector.heartbeat(); // 5000
    assert!(detector.is_available()); // 5500
    detector.heartbeat(); // 6000
    assert!(!detector.is_available()); // 11000
}

#[test]
fn node_heartbeat_missed_alive() {
    let intervals = vec![0, 1000, 1000, 1000, 4000, 1000, 1000];
    let detector = builder()
        .acceptable_heartbeat_pause(Duration::from_secs(3))
        .clock(FakeClock::new(intervals))
        .build()
        .unwrap();

    detector.heartbeat(); // 0
    detector.heartbeat(); // 1000
    detector.heartbeat(); // 2000
    detector.heartbeat(); // 3000
    assert!(detector.is_available()); // 7000
    detector.heartbeat(); // 8000
    assert!(detector.is_available()); // 9000
}

#[test]
fn dead_node_alive_again() {
    let intervals = vec![0, 1000, 1000, 1000, 3000, 1000, 1000];
    let detector = builder().clock(FakeClock::new(intervals)).build().unwrap();

    detector.heartbeat(); // 0
    detector.heartbeat(); // 1000
    detector.heartbeat(); // 2000
    detector.heartbeat(); // 3000
    assert!(!detector.is_available()); // 6000
    detector.heartbeat(); // 7000
    assert!(detector.is_available()); // 8000
}

#[test]
fn node_heartbeat_missed_dead_real_clock() {
    let detector = builder().build().unwrap();

    detector.heartbeat(); // 0
    thread::sleep(Duration::from_millis(1000));
    detector.heartbeat(); // 1000
    thread::sleep(Duration::from_millis(100));
    detector.heartbeat(); // 1100
    thread::sleep(Duration::from_millis(100));

    assert!(detector.is_available()); // 1200
    thread::sleep(Duration::from_millis(7000));
    assert!(!detector.is_available()); // 8200
}
