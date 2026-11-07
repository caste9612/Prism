//! Frames of a video through Media Foundation (ADR-0013): the codecs installed in Windows decode
//! them, nothing is bundled. Used for the prints of the similar videos (F12).
//!
//! The source reader delivers the first video stream as NV12 when the decoder can (the usual
//! output of H.264, HEVC, VP9 decoders: no conversion at all), otherwise as 32-bit RGB. Only
//! the brightness is kept: the luma plane of NV12, or the luma computed from RGB. For each
//! requested point of the timeline the reader seeks there (landing on the previous key frame)
//! and decodes at most `max_after_seek` more frames to get closer to the requested time.

use std::path::Path;

use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer, IMFAttributes, IMFMediaBuffer, IMFMediaType, IMFSample, IMFSourceReader,
    MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_PD_DURATION,
    MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MF_SOURCE_READER_MEDIASOURCE, MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION, MFCreateAttributes,
    MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Video, MFSTARTUP_LITE, MFShutdown,
    MFStartup, MFVideoFormat_NV12, MFVideoFormat_RGB32,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::core::{GUID, HSTRING, Interface};

#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    #[error("Media Foundation: {0}")]
    MediaFoundation(#[from] windows::core::Error),
    #[error("the video has no duration")]
    NoDuration,
    #[error("no frame could be decoded")]
    NoFrame,
}

/// A decoded frame as grey levels, top row first, one byte per pixel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub luma: Vec<u8>,
}

/// Frames at the requested points of a video, with its duration and size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoFrames {
    /// In milliseconds.
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub frames: Vec<Frame>,
}

/// Media Foundation started on this thread for as long as the guard lives.
struct MfSession {
    com: bool,
    _not_send: std::marker::PhantomData<*const ()>,
}

impl MfSession {
    fn start() -> Result<Self, VideoError> {
        // SAFETY: initialises COM for the calling thread; balanced in `drop` when it succeeded
        // (an already initialised thread returns S_FALSE, which also needs the uninitialise).
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        // SAFETY: MFStartup has no pointer arguments; balanced by MFShutdown in `drop`.
        if let Err(e) = unsafe { MFStartup(MF_VERSION, MFSTARTUP_LITE) } {
            if com {
                // SAFETY: balances the successful CoInitializeEx above.
                unsafe { CoUninitialize() };
            }
            return Err(e.into());
        }
        Ok(Self {
            com,
            _not_send: std::marker::PhantomData,
        })
    }
}

impl Drop for MfSession {
    fn drop(&mut self) {
        // SAFETY: balances the MFStartup of `start`, on the same thread (the guard is not Send).
        let _ = unsafe { MFShutdown() };
        if self.com {
            // SAFETY: balances the successful CoInitializeEx of `start`.
            unsafe { CoUninitialize() };
        }
    }
}

const VIDEO_STREAM: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pixels {
    Nv12,
    Rgb32,
}

fn output(reader: &IMFSourceReader, subtype: &GUID) -> Result<(), VideoError> {
    // SAFETY: creates an empty media type, then plain COM calls on valid interfaces.
    unsafe {
        let media_type: IMFMediaType = MFCreateMediaType()?;
        media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        media_type.SetGUID(&MF_MT_SUBTYPE, subtype)?;
        reader.SetCurrentMediaType(VIDEO_STREAM, None, &media_type)?;
    }
    Ok(())
}

fn open(path: &Path) -> Result<(IMFSourceReader, Pixels), VideoError> {
    let mut attributes: Option<IMFAttributes> = None;
    // SAFETY: `attributes` receives a new attribute store.
    unsafe { MFCreateAttributes(&raw mut attributes, 1) }?;
    let attributes = attributes.ok_or(VideoError::NoFrame)?;
    // SAFETY: plain COM calls on valid interfaces; the URL is a NUL-terminated HSTRING.
    let reader = unsafe {
        attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
        let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), &attributes)?;
        reader.SetStreamSelection(VIDEO_STREAM, true)?;
        reader
    };
    let pixels = if output(&reader, &MFVideoFormat_NV12).is_ok() {
        Pixels::Nv12
    } else {
        output(&reader, &MFVideoFormat_RGB32)?;
        Pixels::Rgb32
    };
    Ok((reader, pixels))
}

/// Luma of a locked frame: the Y plane of NV12, or the grey level of each RGB32 pixel.
fn luma(
    base: *const u8,
    pitch: i32,
    len: usize,
    width: u32,
    height: u32,
    pixels: Pixels,
) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut out = vec![0u8; w * h];
    let step = pitch.unsigned_abs() as usize;
    for y in 0..h {
        // A negative pitch means the rows are stored bottom-up from `base`.
        let offset = if pitch < 0 {
            -(y as isize) * step as isize
        } else {
            (y * step) as isize
        };
        let row_len = if pixels == Pixels::Nv12 { w } else { w * 4 };
        // With a positive pitch the rows must lie inside the locked length; a negative pitch
        // comes from `Lock2D`, which guarantees `height` rows.
        if pitch >= 0 && offset as usize + row_len > len {
            break;
        }
        // SAFETY: the caller locked a buffer of `len` bytes (positive pitch) or a 2D buffer
        // whose rows are `pitch` apart starting at `base`; each row read is `row_len` bytes.
        let row = unsafe { std::slice::from_raw_parts(base.offset(offset), row_len) };
        let dst = &mut out[y * w..(y + 1) * w];
        match pixels {
            Pixels::Nv12 => dst.copy_from_slice(row),
            Pixels::Rgb32 => {
                for (d, p) in dst.iter_mut().zip(row.chunks_exact(4)) {
                    // BT.601 weights, B G R order.
                    let v =
                        (u32::from(p[2]) * 77 + u32::from(p[1]) * 150 + u32::from(p[0]) * 29) >> 8;
                    *d = v as u8;
                }
            }
        }
    }
    out
}

fn copy_frame(
    sample: &IMFSample,
    width: u32,
    height: u32,
    pixels: Pixels,
) -> Result<Frame, VideoError> {
    // SAFETY: the sample is valid; its buffer is locked while read and unlocked right after.
    unsafe {
        let buffer: IMFMediaBuffer = sample.ConvertToContiguousBuffer()?;
        let luma = if let Ok(two_d) = buffer.cast::<IMF2DBuffer>() {
            let mut scan0: *mut u8 = std::ptr::null_mut();
            let mut pitch = 0i32;
            two_d.Lock2D(&raw mut scan0, &raw mut pitch)?;
            let len = pitch.unsigned_abs() as usize * height as usize;
            let out = luma(scan0, pitch, len, width, height, pixels);
            two_d.Unlock2D()?;
            out
        } else {
            let mut data: *mut u8 = std::ptr::null_mut();
            let mut len = 0u32;
            buffer.Lock(&raw mut data, None, Some(&raw mut len))?;
            let pitch = i32::try_from(if pixels == Pixels::Nv12 {
                width
            } else {
                width * 4
            })
            .unwrap_or(i32::MAX);
            let out = luma(data, pitch, len as usize, width, height, pixels);
            buffer.Unlock()?;
            out
        };
        Ok(Frame {
            width,
            height,
            luma,
        })
    }
}

/// Decodes the frames at `fractions` (0.0–1.0) of the duration of the video at `path`, each
/// after at most `max_after_seek` frames past the key frame the seek lands on.
///
/// # Errors
/// [`VideoError`] when Media Foundation cannot open or decode the file (format or codec not
/// installed), or the video has no duration.
pub fn sample_frames(
    path: &Path,
    fractions: &[f64],
    max_after_seek: usize,
) -> Result<VideoFrames, VideoError> {
    let _session = MfSession::start()?;
    let (reader, pixels) = open(path)?;
    // SAFETY: plain COM calls on a valid reader; the PROPVARIANT is owned here.
    let (duration, width, height) = unsafe {
        let d = reader
            .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)?;
        let duration = u64::try_from(&d).map_err(|_| VideoError::NoDuration)?;
        let current = reader.GetCurrentMediaType(VIDEO_STREAM)?;
        let size = current.GetUINT64(&MF_MT_FRAME_SIZE)?;
        (duration, (size >> 32) as u32, (size & 0xFFFF_FFFF) as u32)
    };
    if duration == 0 {
        return Err(VideoError::NoDuration);
    }
    let mut frames = Vec::with_capacity(fractions.len());
    for &f in fractions {
        let target = (duration as f64 * f.clamp(0.0, 1.0)) as i64;
        let position = PROPVARIANT::from(target);
        // SAFETY: plain COM calls; the outputs are locals that outlive the calls.
        let frame = unsafe {
            reader.SetCurrentPosition(&GUID::zeroed(), &position)?;
            let mut found = None;
            for _ in 0..=max_after_seek {
                let mut flags = 0u32;
                let mut timestamp = 0i64;
                let mut sample: Option<IMFSample> = None;
                reader.ReadSample(
                    VIDEO_STREAM,
                    0,
                    None,
                    Some(&raw mut flags),
                    Some(&raw mut timestamp),
                    Some(&raw mut sample),
                )?;
                if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                    break;
                }
                if let Some(s) = sample {
                    found = Some(s);
                    if timestamp >= target {
                        break;
                    }
                }
            }
            found
        };
        if let Some(sample) = frame {
            frames.push(copy_frame(&sample, width, height, pixels)?);
        }
    }
    if frames.is_empty() {
        return Err(VideoError::NoFrame);
    }
    Ok(VideoFrames {
        duration_ms: duration / 10_000,
        width,
        height,
        frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Media::MediaFoundation::{
        IMFSinkWriter, MF_MT_AVG_BITRATE, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE,
        MF_MT_INTERLACE_MODE, MF_MT_PIXEL_ASPECT_RATIO, MFCreateMemoryBuffer, MFCreateSample,
        MFCreateSinkWriterFromURL, MFVideoFormat_H264, MFVideoFormat_WMV3,
        MFVideoInterlace_Progressive,
    };

    const FPS: u32 = 25;
    const SECONDS: u32 = 4;

    /// A synthetic clip: a gradient drifting over time and a disc crossing the frame.
    fn pixel(frame: u32, x: u32, y: u32, w: u32, h: u32, alt: bool) -> [u8; 4] {
        let t = frame as f32 / (FPS * SECONDS) as f32;
        let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
        let (cx, cy) = (0.15 + 0.7 * t, 0.5 + 0.25 * (t * 6.0).sin());
        let disc = (fx - cx).powi(2) + (fy - cy).powi(2) < 0.02;
        let base = if alt {
            if ((x * 8 / w) + (y * 6 / h) + frame / 20).is_multiple_of(2) {
                220.0
            } else {
                30.0
            }
        } else {
            255.0 * ((fx + t) % 1.0)
        };
        let v = if disc { 255 - base as u8 } else { base as u8 };
        [v, (255.0 * fy) as u8, v / 2, 0]
    }

    /// Encodes the clip with the encoders of Windows (H.264 in MP4, or WMV9).
    fn write_clip(path: &Path, w: u32, h: u32, bitrate: u32, subtype: &GUID, alt: bool) {
        let _session = MfSession::start().unwrap();
        // SAFETY: plain COM calls on valid interfaces; the buffer is locked while written.
        unsafe {
            let writer: IMFSinkWriter =
                MFCreateSinkWriterFromURL(&HSTRING::from(path.as_os_str()), None, None).unwrap();
            let size = (u64::from(w) << 32) | u64::from(h);
            let rate = (u64::from(FPS) << 32) | 1;
            let out: IMFMediaType = MFCreateMediaType().unwrap();
            out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).unwrap();
            out.SetGUID(&MF_MT_SUBTYPE, subtype).unwrap();
            out.SetUINT32(&MF_MT_AVG_BITRATE, bitrate).unwrap();
            out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                .unwrap();
            out.SetUINT64(&MF_MT_FRAME_SIZE, size).unwrap();
            out.SetUINT64(&MF_MT_FRAME_RATE, rate).unwrap();
            out.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1 << 32) | 1)
                .unwrap();
            let stream = writer.AddStream(&out).unwrap();
            let input: IMFMediaType = MFCreateMediaType().unwrap();
            input
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .unwrap();
            input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).unwrap();
            input
                .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                .unwrap();
            input.SetUINT32(&MF_MT_DEFAULT_STRIDE, w * 4).unwrap();
            input.SetUINT64(&MF_MT_FRAME_SIZE, size).unwrap();
            input.SetUINT64(&MF_MT_FRAME_RATE, rate).unwrap();
            input
                .SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1 << 32) | 1)
                .unwrap();
            writer.SetInputMediaType(stream, &input, None).unwrap();
            writer.BeginWriting().unwrap();
            let len = w * h * 4;
            let duration = 10_000_000 / i64::from(FPS);
            for frame in 0..FPS * SECONDS {
                let buffer = MFCreateMemoryBuffer(len).unwrap();
                let mut data: *mut u8 = std::ptr::null_mut();
                buffer.Lock(&raw mut data, None, None).unwrap();
                let bytes = std::slice::from_raw_parts_mut(data, len as usize);
                for y in 0..h {
                    for x in 0..w {
                        let at = ((y * w + x) * 4) as usize;
                        bytes[at..at + 4].copy_from_slice(&pixel(frame, x, y, w, h, alt));
                    }
                }
                buffer.Unlock().unwrap();
                buffer.SetCurrentLength(len).unwrap();
                let sample = MFCreateSample().unwrap();
                sample.AddBuffer(&buffer).unwrap();
                sample.SetSampleTime(i64::from(frame) * duration).unwrap();
                sample.SetSampleDuration(duration).unwrap();
                writer.WriteSample(stream, &sample).unwrap();
            }
            writer.Finalize().unwrap();
        }
    }

    fn prints(path: &Path) -> (u64, Vec<u64>) {
        let v = sample_frames(path, &[0.1, 0.3, 0.5, 0.7, 0.9], 300).unwrap();
        let p = v
            .frames
            .iter()
            .map(|f| prism_core::media::dhash_luma(f.width, f.height, &f.luma).unwrap())
            .collect();
        (v.duration_ms, p)
    }

    fn mean_distance(a: &[u64], b: &[u64]) -> f64 {
        let total: u32 = a.iter().zip(b).map(|(x, y)| (x ^ y).count_ones()).sum();
        f64::from(total) / a.len() as f64
    }

    // ADR-0013, F12-AC1/AC2: the same clip encoded at another size and bitrate, and in WMV,
    // gives nearly the same frame prints; another clip does not.
    #[test]
    fn reencoded_videos_keep_their_prints() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("clip.mp4");
        let small = dir.path().join("clip-small.mp4");
        let wmv = dir.path().join("clip.wmv");
        let other = dir.path().join("other.mp4");
        write_clip(&big, 640, 360, 2_000_000, &MFVideoFormat_H264, false);
        write_clip(&small, 320, 180, 300_000, &MFVideoFormat_H264, false);
        write_clip(&wmv, 480, 270, 800_000, &MFVideoFormat_WMV3, false);
        write_clip(&other, 640, 360, 2_000_000, &MFVideoFormat_H264, true);

        let (d_big, p_big) = prints(&big);
        assert!((3_900..=4_100).contains(&d_big), "duration {d_big} ms");
        assert_eq!(p_big.len(), 5);
        let (_, p_small) = prints(&small);
        let (_, p_wmv) = prints(&wmv);
        let (_, p_other) = prints(&other);
        let near = mean_distance(&p_big, &p_small);
        let wmv_near = mean_distance(&p_big, &p_wmv);
        let far = mean_distance(&p_big, &p_other);
        println!("mean frame distance: smaller {near:.1}, wmv {wmv_near:.1}, other {far:.1}");
        assert!(near <= 4.0, "re-encoded: {near}");
        assert!(wmv_near <= 4.0, "wmv: {wmv_near}");
        assert!(far >= 16.0, "other clip: {far}");
        // A file that is not a video.
        let text = dir.path().join("notes.mp4");
        std::fs::write(&text, b"not a video").unwrap();
        assert!(sample_frames(&text, &[0.5], 0).is_err());
    }
}
