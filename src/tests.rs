use {
    super::*,
    std::{
        sync::atomic::{AtomicU64, AtomicUsize, Ordering},
        thread,
    },
};

#[test]
fn circle_buffer() {
    let mut buf = CircleBuffer::new(3);

    assert_eq!(buf.len(), 0);
    assert_eq!(buf.push(1), None);
    assert_eq!(buf.len(), 1);
    assert_eq!(buf.push(2), None);
    assert_eq!(buf.len(), 2);
    assert_eq!(buf.push(3), None);
    assert_eq!(buf.len(), 3);
    assert_eq!(buf.push(4), Some(1));
    assert_eq!(buf.len(), 4);
    assert_eq!(buf.push(5), Some(2));
    assert_eq!(buf.len(), 5);
    assert_eq!(buf.push(6), Some(3));
    assert_eq!(buf.len(), 6);
    assert_eq!(buf.push(7), Some(4));
    assert_eq!(buf.len(), 7);
}

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

    fn elapsed(&self, before: &Self::Timestamp, after: &Self::Timestamp) -> Duration {
        Duration::from_millis(*after - *before)
    }
}

fn create_config() -> Config {
    Config {
        threshold: 8.0,
        max_sample_size: 1000,
        min_std_deviation: Duration::from_millis(10),
        acceptable_heartbeat_pause: Duration::ZERO,
        first_heartbeat_estimate: Duration::from_secs(1),
    }
}

fn create_fake_detector(intervals: Vec<u64>) -> Detector<FakeClock> {
    create_detector_with_config(intervals, create_config())
}

fn create_detector_with_config(intervals: Vec<u64>, config: Config) -> Detector<FakeClock> {
    Detector::new(config, FakeClock::new(intervals))
}

#[test]
fn node_available() {
    let intervals = vec![0, 1000, 100, 100];
    let mut detector = create_fake_detector(intervals);
    detector.heartbeat();
    detector.heartbeat();
    detector.heartbeat();
    assert!(detector.is_available());
}

#[test]
fn node_heartbeat_missed_dead1() {
    let intervals = vec![0, 1000, 100, 100, 7000];
    let mut detector = create_fake_detector(intervals);

    detector.heartbeat(); // 0
    detector.heartbeat(); // 1000
    detector.heartbeat(); // 1100

    assert!(detector.is_available()); // 1200
    assert!(!detector.is_available()); // 8200
}

#[test]
fn node_heartbeat_missed_dead2() {
    let intervals = vec![0, 1000, 1000, 1000, 1000, 1000, 500, 500, 5000];
    let mut config = create_config();
    config.acceptable_heartbeat_pause = Duration::from_secs(3);
    let mut detector = create_detector_with_config(intervals, config);

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
    let mut config = create_config();
    config.acceptable_heartbeat_pause = Duration::from_secs(3);
    let mut detector = create_detector_with_config(intervals, config);

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
    let mut detector = create_fake_detector(intervals);

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
    let config = create_config();
    let mut detector = Detector::new(config, DefaultClock);

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
