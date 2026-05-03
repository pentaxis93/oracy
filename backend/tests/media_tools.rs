use std::process::Command;

use oracy_backend::transcription_worker::{
    AudioSlicer, DurationProbe, FfmpegAudioSlicer, FfprobeDurationProbe,
};
use tempfile::TempDir;

#[tokio::test]
async fn ffprobe_derives_duration_and_ffmpeg_creates_format_safe_slices() {
    let tempdir = TempDir::new().expect("tempdir");
    let audio = tempdir.path().join("one-second.wav");
    let output = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg("anullsrc=r=8000:cl=mono")
        .arg("-t")
        .arg("1.0")
        .arg(&audio)
        .output()
        .expect("run ffmpeg");
    assert!(
        output.status.success(),
        "ffmpeg fixture generation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let duration_ms = FfprobeDurationProbe
        .duration_ms(&audio, "wav")
        .await
        .expect("probe duration");
    assert!((900..=1_100).contains(&duration_ms));

    let slices = FfmpegAudioSlicer::with_max_slice_bytes(8_000)
        .slices(&audio, "wav")
        .await
        .expect("slice audio");
    assert!(slices.len() > 1);
    for slice in slices {
        assert_ne!(slice, audio);
        assert!(slice.exists());
        let slice_size = std::fs::metadata(slice).expect("slice metadata").len();
        assert!(slice_size > 0);
        assert!(slice_size <= 8_000);
    }
}
