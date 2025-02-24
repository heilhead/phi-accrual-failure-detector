use std::{
    cell::RefCell,
    marker::PhantomData,
    sync::RwLock,
    time::{Duration, Instant},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Threshold must be > 0")]
    Threshold,

    #[error("Max sample size must be > 0")]
    MaxSampleSize,

    #[error("Min standard deviation must be > 0")]
    MinStdDeviation,

    #[error("First heartbeat estimate must be > 0")]
    FirstHeartbeatEstimate,
}

/// [`FailureDetector`] for single-threaded environments.
pub type UnsyncDetector = FailureDetector<UnsyncState<DefaultClock>>;

/// [`FailureDetector`] for multi-threaded environments.
pub type SyncDetector = FailureDetector<SyncState<DefaultClock>>;

/// [`FailureDetector`] builder.
pub struct Builder<S: sealed::State> {
    config: Config,
    clock: S::Clock,
    _marker: PhantomData<S>,
}

impl<S: sealed::State<Clock = DefaultClock>> Builder<S> {
    pub fn new() -> Self {
        Self {
            config: Default::default(),
            clock: DefaultClock,
            _marker: PhantomData,
        }
    }
}

impl Default for Builder<UnsyncState<DefaultClock>> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: sealed::State> Builder<S> {
    /// Threshold for considering the monitored resource unavailable.
    ///
    /// A low threshold is prone to generate many wrong suspicions but ensures a
    /// quick detection in the event of a real crash. Conversely, a high
    /// threshold generates fewer mistakes but needs more time to detect actual
    /// crashes.
    ///
    /// Default: 8.0
    pub fn threshold(mut self, threshold: f64) -> Self {
        self.config.threshold = threshold;
        self
    }

    /// Number of samples to use for calculation of mean and standard deviation
    /// of inter-arrival times.
    ///
    /// Default: 100
    pub fn max_sample_size(mut self, max_sample_size: usize) -> Self {
        self.config.max_sample_size = max_sample_size;
        self
    }

    /// Minimum standard deviation to use for the normal distribution used when
    /// calculating phi. Too low standard deviation might result in too much
    /// sensitivity for sudden, but normal, deviations in heartbeat inter
    /// arrival times.
    ///
    /// Default: 100ms
    pub fn min_std_deviation(mut self, min_std_deviation: Duration) -> Self {
        self.config.min_std_deviation = min_std_deviation;
        self
    }

    /// Duration corresponding to number of potentially lost/delayed heartbeats
    /// that will be accepted before considering it to be an anomaly. This
    /// margin is important to be able to survive sudden, occasional, pauses in
    /// heartbeat   arrivals, due to for example garbage collect or network
    /// drop.
    ///
    /// Default: 3s
    pub fn acceptable_heartbeat_pause(mut self, acceptable_heartbeat_pause: Duration) -> Self {
        self.config.acceptable_heartbeat_pause = acceptable_heartbeat_pause;
        self
    }

    /// Bootstrap the stats with heartbeats that corresponds to to this
    /// duration, with a with rather high standard deviation (since environment
    /// is unknown in the beginning).
    ///
    /// Default: 1s
    pub fn first_heartbeat_estimate(mut self, first_heartbeat_estimate: Duration) -> Self {
        self.config.first_heartbeat_estimate = first_heartbeat_estimate;
        self
    }

    /// Use [`RwLock`] internally to make the detector [`Sync`].
    pub fn sync(self) -> Builder<SyncState<S::Clock>> {
        self.state::<SyncState<S::Clock>>()
    }

    /// Use [`RefCell`] internally instead of [`RwLock`] for slightly better
    /// performance.
    pub fn unsync(self) -> Builder<UnsyncState<S::Clock>> {
        self.state::<UnsyncState<S::Clock>>()
    }

    /// Provide an alternative implementation of [`Clock`].
    ///
    /// Default: [`DefaultClock`]
    pub fn clock<T: Clock>(self, clock: T) -> Builder<S::WithClock<T>> {
        Builder {
            config: self.config,
            clock,
            _marker: PhantomData,
        }
    }

    /// Builds an instance of [`Detector`].
    ///
    /// Returns an [`Error`] if some configuration parameters are incorrect.
    pub fn build(self) -> Result<FailureDetector<S>, Error> {
        let config = self.config;

        if config.threshold <= 0. {
            return Err(Error::Threshold);
        }

        if config.max_sample_size == 0 {
            return Err(Error::MaxSampleSize);
        }

        if config.min_std_deviation.is_zero() {
            return Err(Error::MinStdDeviation);
        }

        if config.first_heartbeat_estimate.is_zero() {
            return Err(Error::FirstHeartbeatEstimate);
        }

        let mean = config.first_heartbeat_estimate.as_millis() as f64;
        let std_deviation = mean / 4.;

        let threshold = config.threshold;
        let acceptable_heartbeat_pause = config.acceptable_heartbeat_pause.as_millis() as f64;
        let min_std_deviation = config.min_std_deviation.as_millis() as f64;

        let mut history = HeartbeatHistory::new(config.max_sample_size);
        history.add(mean - std_deviation);
        history.add(mean + std_deviation);

        let state = DetectorState {
            threshold,
            acceptable_heartbeat_pause,
            min_std_deviation,
            history,
            last_timestamp: None,
        };

        Ok(FailureDetector {
            state: state.into(),
            clock: self.clock,
        })
    }

    fn state<T: sealed::State<Clock = S::Clock>>(self) -> Builder<T> {
        Builder {
            config: self.config,
            clock: self.clock,
            _marker: PhantomData,
        }
    }
}

struct Config {
    threshold: f64,
    max_sample_size: usize,
    min_std_deviation: Duration,
    acceptable_heartbeat_pause: Duration,
    first_heartbeat_estimate: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            threshold: 8.0,
            max_sample_size: 100,
            min_std_deviation: Duration::from_millis(100),
            acceptable_heartbeat_pause: Duration::from_secs(3),
            first_heartbeat_estimate: Duration::from_secs(1),
        }
    }
}

struct DetectorState<C: Clock> {
    threshold: f64,
    acceptable_heartbeat_pause: f64,
    min_std_deviation: f64,
    history: HeartbeatHistory,
    last_timestamp: Option<C::Timestamp>,
}

impl<C: Clock> DetectorState<C> {
    fn heartbeat(&mut self, timestamp: C::Timestamp) {
        if let (Some(last_timestamp), true) = (
            &self.last_timestamp,
            self.is_available_for_timestamp(&timestamp),
        ) {
            self.history.add(C::elapsed_ms(last_timestamp, &timestamp));
        }

        self.last_timestamp = Some(timestamp);
    }

    fn is_available_for_timestamp(&self, timestamp: &C::Timestamp) -> bool {
        self.phi_for_timestamp(timestamp) < self.threshold
    }

    fn phi_for_timestamp(&self, timestamp: &C::Timestamp) -> f64 {
        let Some(last_timestamp) = &self.last_timestamp else {
            // No heartbeats received yet.
            return 0.0;
        };

        let time_diff = C::elapsed_ms(last_timestamp, timestamp);
        let mean = self.history.mean() + self.acceptable_heartbeat_pause;
        let std_deviation = self.history.std_deviation().max(self.min_std_deviation);

        let y = (time_diff - mean) / std_deviation;
        let e = (-y * (1.5976 + 0.070566 * y * y)).exp();

        if time_diff > mean {
            -(e / (1.0 + e)).log10()
        } else {
            -(1.0 - 1.0 / (1.0 + e)).log10()
        }
    }
}

/// Implementation of 'The Phi Accrual Failure Detector' by Hayashibara et al.
/// as defined in their paper: <https://oneofus.la/have-emacs-will-hack/files/HDY04.pdf>
///
/// The suspicion level of failure is given by a value called φ (phi). The basic
/// idea of the φ failure detector is to express the value of φ on a scale that
/// is dynamically adjusted to reflect current network conditions. A
/// configurable threshold is used to decide if φ is considered to be a failure.
///
/// The value of φ is calculated as: `φ = -log10(1 - F(timeSinceLastHeartbeat)`
/// where `F` is the cumulative distribution function of a normal distribution
/// with mean and standard deviation estimated from historical heartbeat
/// inter-arrival times.
pub struct FailureDetector<S: sealed::State> {
    state: S,
    clock: S::Clock,
}

impl<S: sealed::State<Clock = DefaultClock>> FailureDetector<S> {
    pub fn builder() -> Builder<S> {
        Builder::new()
    }
}

impl<S: sealed::State<Clock = DefaultClock>> Default for FailureDetector<S> {
    fn default() -> Self {
        // Safe unwrap with default parameters.
        Self::builder().build().unwrap()
    }
}

pub trait Detector {
    /// Notifies the detector that a heartbeat arrived from the monitored
    /// resource. This causes the detector to update its state.
    fn heartbeat(&self);

    /// The suspicion level of the accrual failure detector.
    ///
    /// If a connection does not have any records in failure detector then it is
    /// considered healthy.
    fn phi(&self) -> f64;

    /// Returns `true` if the resource is considered to be up and healthy and
    /// returns `false` otherwise.
    fn is_available(&self) -> bool;
}

/// A [`FailureDetector`] state wrapper based on [`RefCell`] for single-threaded
/// access.
pub struct UnsyncState<C: Clock>(RefCell<DetectorState<C>>);

impl<C: Clock> sealed::State for UnsyncState<C> {
    type Clock = C;
    type WithClock<T: Clock> = UnsyncState<T>;
}

impl<C: Clock> From<DetectorState<C>> for UnsyncState<C> {
    fn from(inner: DetectorState<C>) -> Self {
        Self(RefCell::new(inner))
    }
}

impl<C: Clock> Detector for FailureDetector<UnsyncState<C>> {
    fn heartbeat(&self) {
        self.state.0.borrow_mut().heartbeat(self.clock.timestamp());
    }

    fn phi(&self) -> f64 {
        self.state
            .0
            .borrow()
            .phi_for_timestamp(&self.clock.timestamp())
    }

    fn is_available(&self) -> bool {
        self.state
            .0
            .borrow()
            .is_available_for_timestamp(&self.clock.timestamp())
    }
}

/// A [`FailureDetector`] state wrapper based on [`RwLock`] for multi-threaded
/// access.
pub struct SyncState<C: Clock>(RwLock<DetectorState<C>>);

impl<C: Clock> sealed::State for SyncState<C> {
    type Clock = C;
    type WithClock<T: Clock> = SyncState<T>;
}

impl<C: Clock> From<DetectorState<C>> for SyncState<C> {
    fn from(inner: DetectorState<C>) -> Self {
        Self(RwLock::new(inner))
    }
}

impl<C: Clock> Detector for FailureDetector<SyncState<C>> {
    fn heartbeat(&self) {
        self.state
            .0
            .write()
            .unwrap()
            .heartbeat(self.clock.timestamp());
    }

    fn phi(&self) -> f64 {
        self.state
            .0
            .read()
            .unwrap()
            .phi_for_timestamp(&self.clock.timestamp())
    }

    fn is_available(&self) -> bool {
        self.state
            .0
            .read()
            .unwrap()
            .is_available_for_timestamp(&self.clock.timestamp())
    }
}

mod sealed {
    use super::*;

    #[allow(private_bounds)]
    pub trait State: From<DetectorState<Self::Clock>> {
        type Clock: Clock;
        type WithClock<T: Clock>: State<Clock = T>;
    }
}

pub trait Clock {
    type Timestamp;

    /// Returns current time.
    fn timestamp(&self) -> Self::Timestamp;

    /// Returns time elapsed between two timestamps.
    fn elapsed(before: &Self::Timestamp, after: &Self::Timestamp) -> Duration;

    fn elapsed_ms(before: &Self::Timestamp, after: &Self::Timestamp) -> f64 {
        Self::elapsed(before, after).as_millis() as f64
    }
}

/// The default clock implementation based on using [`std::time::Instant`].
pub struct DefaultClock;

impl Clock for DefaultClock {
    type Timestamp = Instant;

    fn timestamp(&self) -> Self::Timestamp {
        Instant::now()
    }

    fn elapsed(before: &Self::Timestamp, after: &Self::Timestamp) -> Duration {
        if before > after {
            Duration::ZERO
        } else {
            after.duration_since(*before)
        }
    }
}

/// Holds the heartbeat statistics for a specific node Address. It is capped by
/// the number of samples specified in `max_sample_size`.
///
/// The stats (`mean`, `variance`, `std_deviation`) are not defined for empty
/// [`HeartbeatHistory`].
struct HeartbeatHistory {
    intervals: CircleBuffer<f64>,
    interval_sum: f64,
    squared_interval_sum: f64,
}

impl HeartbeatHistory {
    fn new(max_sample_size: usize) -> Self {
        assert!(max_sample_size > 0);

        Self {
            intervals: CircleBuffer::new(max_sample_size),
            interval_sum: 0.,
            squared_interval_sum: 0.,
        }
    }

    fn mean(&self) -> f64 {
        self.interval_sum / self.intervals.len() as f64
    }

    fn variance(&self) -> f64 {
        self.squared_interval_sum / self.intervals.len() as f64 - pow2(self.mean())
    }

    fn std_deviation(&self) -> f64 {
        self.variance().sqrt()
    }

    fn add(&mut self, interval: f64) {
        self.interval_sum += interval;
        self.squared_interval_sum += pow2(interval);

        if let Some(oldest) = self.intervals.push(interval) {
            self.interval_sum -= oldest;
            self.squared_interval_sum -= pow2(oldest);
        }
    }
}

#[inline]
fn pow2(x: f64) -> f64 {
    x * x
}

/// Simple circular buffer that only allows for pushing values, and returns the
/// oldest value on overflow.
#[derive(Clone)]
struct CircleBuffer<T> {
    data: Vec<T>,
    capacity: usize,
    cursor: usize,
}

impl<T> CircleBuffer<T> {
    fn new(capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            data: Vec::with_capacity(capacity),
            capacity,
            cursor: 0,
        }
    }

    fn push(&mut self, item: T) -> Option<T> {
        self.cursor += 1;

        if self.data.len() < self.capacity {
            self.data.push(item);

            None
        } else {
            let oldest_idx = (self.cursor - 1) % self.capacity;

            Some(std::mem::replace(&mut self.data[oldest_idx], item))
        }
    }

    fn len(&self) -> usize {
        self.cursor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn ensure_sync<T: Sync>() {}

    #[test]
    fn ensure_bounds() {
        ensure_sync::<SyncDetector>();
        let _: SyncDetector = UnsyncDetector::builder().sync().build().unwrap();
        let _: UnsyncDetector = SyncDetector::builder().unsync().build().unwrap();
    }
}
