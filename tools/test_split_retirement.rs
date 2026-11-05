//! Direct-rustc ownership proof; uses the production retirement poll and skin IO pool.
//! rustc --edition 2024 --test tools/test_split_retirement.rs -o <private-temp>/proof
#[path = "../crates/horizon-client/src/game/split/retired/cohort.rs"]
mod cohort;
#[path = "../crates/horizon-client-render/src/skins/workers.rs"]
mod workers;

#[cfg(test)]
mod tests {
    use super::{cohort, workers::Pool};
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        sync::{Arc, atomic::AtomicBool, mpsc},
        time::{Duration, Instant},
    };

    struct Owner {
        pool: Arc<Pool>,
        dropped: mpsc::Sender<()>,
    }
    impl Drop for Owner {
        fn drop(&mut self) {
            self.dropped.send(()).unwrap();
        }
    }

    #[test]
    fn departed_owner_survives_blocked_skin_io_until_owned_join() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut release = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut read, _) = listener.accept().unwrap();
        let pool = Pool::new("split retired skin IO proof", 1);
        let weak = Arc::downgrade(&pool);
        let (entered, entered_rx) = mpsc::channel();
        let (finished, finished_rx) = mpsc::channel();
        pool.submit(
            Arc::new(AtomicBool::new(false)),
            move |_, _| {
                entered.send(()).unwrap();
                let mut byte = [0];
                read.read_exact(&mut byte).unwrap();
                assert_eq!(byte, [42]);
                finished.send(()).unwrap();
            },
            || panic!("active IO must finish before its owner is destroyed"),
        );
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let (dropped, dropped_rx) = mpsc::channel();
        let (responsive, responsive_rx) = mpsc::channel();
        let (resume, resume_rx) = mpsc::channel();
        let ui = std::thread::spawn(move || {
            let mut retired = vec![Owner { pool, dropped }];
            // Ordinary frames and Quit both retain the owner while real IO is blocked.
            for _ in 0..3 {
                assert!(!cohort::poll(&mut retired, |owner| owner.pool.shutdown()));
                assert_eq!(retired.len(), 1);
            }
            responsive.send(()).unwrap();
            resume_rx.recv().unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            while !cohort::poll(&mut retired, |owner| owner.pool.shutdown()) {
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert!(retired.is_empty());
        });
        // A regression may block; always release the TCP read before failing the assertion.
        let responsive = responsive_rx
            .recv_timeout(Duration::from_millis(250))
            .is_ok();
        let kept_owner = weak.strong_count() == 1 && dropped_rx.try_recv().is_err();
        let still_blocked = finished_rx.try_recv().is_err();
        release.write_all(&[42]).unwrap();
        resume.send(()).unwrap();
        ui.join().unwrap();
        assert!(
            responsive,
            "retirement blocked the UI while skin IO was active"
        );
        assert!(
            kept_owner,
            "retirement lost ownership of an active skin service"
        );
        assert!(still_blocked, "the fixture did not hold real IO active");
        finished_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        dropped_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            weak.upgrade().is_none(),
            "finished skin workers were not reaped"
        );
    }
}
