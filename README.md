# Phi Accrual Failure Detector

A port of [Apache Pekko implementation](https://github.com/apache/pekko/blob/7a0fc83e75127f99b630b40f120430f0a975494c/remote/src/main/scala/org/apache/pekko/remote/PhiAccrualFailureDetector.scala) of [Phi Accrual Failure Detector](https://oneofus.la/have-emacs-will-hack/files/HDY04.pdf).

## Usage

Adding dependency:

```toml
[dependencies]
phi-accrual-failure-detector = "0.1"
```

Example:

```rust
let detector = UnsyncDetector::default();

detector.heartbeat();
thread::sleep(Duration::from_millis(1000));

detector.heartbeat();
thread::sleep(Duration::from_millis(1000));

detector.heartbeat();
thread::sleep(Duration::from_millis(1000));

// The resource is available when receiving regular heartbeats.
println!("is available: {}", detector.is_available());
assert!(detector.is_available());

thread::sleep(Duration::from_millis(4000));

// The resource is no longer available, since it's missed a few heartbeats.
println!("is available: {}", detector.is_available());
assert!(!detector.is_available());
```

# License

[Apache 2.0](LICENSE)
