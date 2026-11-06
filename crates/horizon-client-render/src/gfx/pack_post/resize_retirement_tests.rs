//! Real Metal work with a deliberately delayed retirement acknowledgement.
//! The GPU is not deliberately hung: its callback only sends a token.
use super::*;
use std::io::{Cursor, Write};
use std::sync::{atomic::Ordering, mpsc};
use std::time::{Duration, Instant};

#[test]
fn resize_waits_for_owned_retirement_without_allocating_or_blocking() {
    let Some(gpu) = destination_tests::Fixture::new() else {
        return;
    };
    eprintln!("resize retirement adapter: {:?}", gpu.adapter.get_info());
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, source) in [
        (
            "shaders/final.vsh",
            "#version 120\nvoid main(){gl_Position=ftransform();}",
        ),
        (
            "shaders/final.fsh",
            "#version 120\nvoid main(){gl_FragColor=vec4(0.0,1.0,0.0,1.0);}",
        ),
    ] {
        zip.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(source.as_bytes()).unwrap();
    }
    let pack = horizon_shaders::Pack::from_zip_bytes(
        "original resize control",
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let parsed = horizon_shaders::load_post(
        &pack,
        "",
        &Default::default(),
        &horizon_shaders::Macros::standard(),
        &Default::default(),
    );
    let mut post = gpu.build(parsed);
    let previous_picture = gpu.render(&mut post, [2, 2]);
    assert!(previous_picture.iter().all(|p| *p == [0, 255, 0, 255]));
    assert!(!post.setup_pending);
    let (send, receive) = mpsc::channel();
    let started = Instant::now();
    let state = post
        .resize_with(&gpu.device, [4, 4], |complete| {
            gpu.queue.on_submitted_work_done(move || {
                send.send(complete).unwrap();
            });
        })
        .unwrap();
    assert_eq!(state, ResizeState::Pending);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "retirement must not wait for its acknowledgement"
    );
    assert!(
        post.targets.is_none(),
        "old targets are released before replacement allocation"
    );
    assert!(post.passes.iter().all(|pass| pass.groups.is_empty()));
    assert!(post.complete, "a busy GPU is not an unsupported pack");
    assert_eq!(
        post.resize_with(&gpu.device, [8, 4], |_| panic!(
            "one retirement fence covers coalesced sizes"
        ))
        .unwrap(),
        ResizeState::Pending
    );
    assert_eq!(post.resize_pending.as_ref().unwrap().size, [8, 4]);
    assert!(post.targets.is_none());
    let deadline = Instant::now() + Duration::from_secs(5);
    let completion = loop {
        gpu.device.poll(wgpu::PollType::Poll).unwrap();
        if let Ok(completion) = receive.try_recv() {
            break completion;
        }
        assert!(
            Instant::now() < deadline,
            "Metal did not complete the previous frame"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(!completion.load(Ordering::Acquire));
    assert!(
        post.targets.is_none(),
        "GPU completion alone is not the test's delayed acknowledgement"
    );
    completion.store(true, Ordering::Release);
    assert_eq!(
        post.resize(&gpu.device, &gpu.queue, [8, 4]).unwrap(),
        ResizeState::Ready
    );
    assert_eq!(post.targets.as_ref().unwrap().size, [8, 4]);
    assert!(post.resize_pending.is_none());
    assert!(
        post.setup_pending,
        "resizing reruns the pack's initialization"
    );
    let resized_picture = gpu.render(&mut post, [8, 4]);
    assert!(resized_picture.iter().all(|p| *p == [0, 255, 0, 255]));
}
