//! Beat detection for the Reel window (docs/design/segments-and-reel-mode.md
//! §C): where the beats of a song fall, so clips can be cut on them.
//!
//! "It need not be scientific, it goes by vibes, but it must work for any
//! music" (owner). The classic recipe, small enough to own:
//!
//! 1. Decode to mono 22.05 kHz with the bundled ffmpeg (any format).
//! 2. Onset strength: spectral flux of the log-magnitude spectrum (how much
//!    new energy each ~23 ms frame brings), with its slow trend removed.
//! 3. Tempo: autocorrelation of that envelope, weighted towards ~120 BPM so a
//!    calm song isn't read at half or double speed by accident.
//! 4. Beats: Ellis' dynamic programming tracker (as in librosa): beats land on
//!    strong onsets while keeping close to the tempo's spacing, so a quiet bar
//!    doesn't lose its beats and a stray drum hit doesn't add one.
//! 5. Major beats: the downbeats, taking 4/4 and choosing which of the four
//!    beat phases carries the most bass. The rest are minor beats.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Serialize;

pub const SR: u32 = 22_050;
const N: usize = 1024;
const HOP: usize = 512;

#[derive(Serialize, Clone, Debug, Default)]
pub struct BeatInfo {
    /// Seconds of audio.
    pub duration: f64,
    pub bpm: f64,
    /// Every beat, in seconds, ascending.
    pub beats: Vec<f64>,
    /// The downbeats (a subset of `beats`): the strong cut points.
    pub major: Vec<f64>,
    /// A loudness outline for drawing the song: peak level (0..1) per bucket,
    /// `wave_rate` buckets per second.
    pub wave: Vec<f32>,
    pub wave_rate: f64,
}

/// Decode `path` to mono f32 samples at `SR`.
pub fn decode(ffmpeg: &Path, path: &Path) -> Result<Vec<f32>, String> {
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-v", "error", "-nostdin", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", &SR.to_string(), "-f", "f32le", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| format!("couldn't run ffmpeg: {e}"))?;
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or("no ffmpeg output")?
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let mut err = String::new();
    if let Some(mut s) = child.stderr.take() {
        let _ = s.read_to_string(&mut err);
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if bytes.len() < 4 * SR as usize {
        let why = err.lines().last().unwrap_or("").trim().to_string();
        return Err(if !status.success() && !why.is_empty() {
            format!("couldn't read that audio file ({why})")
        } else {
            "that file has no audio (or under a second of it)".into()
        });
    }
    Ok(bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect())
}

/// In-place iterative radix-2 FFT (re, im), length a power of two.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f32::consts::PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let a = start + k;
                let b = a + len / 2;
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
        }
        len <<= 1;
    }
}

/// Onset strength per frame (all bands) and its bass-only counterpart.
fn onset_envelopes(x: &[f32]) -> (Vec<f32>, Vec<f32>) {
    if x.len() < N {
        return (Vec::new(), Vec::new());
    }
    let frames = (x.len() - N) / HOP + 1;
    let hann: Vec<f32> = (0..N).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N as f32).cos()).collect();
    // Bins up to ~150 Hz carry the kick drum / bass line.
    let bass_hi = ((150.0 * N as f32) / SR as f32).ceil() as usize;
    let mut prev = vec![0f32; N / 2];
    let mut cur = vec![0f32; N / 2];
    let mut re = vec![0f32; N];
    let mut im = vec![0f32; N];
    let mut all = Vec::with_capacity(frames);
    let mut bass = Vec::with_capacity(frames);
    for f in 0..frames {
        let off = f * HOP;
        for i in 0..N {
            re[i] = x[off + i] * hann[i];
            im[i] = 0.0;
        }
        fft(&mut re, &mut im);
        for k in 0..N / 2 {
            cur[k] = (1.0 + 100.0 * (re[k] * re[k] + im[k] * im[k]).sqrt()).ln();
        }
        let (mut a, mut b) = (0f32, 0f32);
        if f > 0 {
            for k in 1..N / 2 {
                let d = cur[k] - prev[k];
                if d > 0.0 {
                    a += d;
                    if k <= bass_hi {
                        b += d;
                    }
                }
            }
        }
        all.push(a);
        bass.push(b);
        std::mem::swap(&mut prev, &mut cur);
    }
    (detrend(&all), detrend(&bass))
}

/// Remove the slow trend (a moving average over ~1 s), keep what rises above
/// it, and scale to unit standard deviation.
fn detrend(v: &[f32]) -> Vec<f32> {
    let w = (SR as usize / HOP).max(1); // ~1 s of frames
    let n = v.len();
    let mut prefix = vec![0f64; n + 1];
    for i in 0..n {
        prefix[i + 1] = prefix[i] + v[i] as f64;
    }
    let mut out: Vec<f32> = (0..n)
        .map(|i| {
            let lo = i.saturating_sub(w / 2);
            let hi = (i + w / 2 + 1).min(n);
            let mean = (prefix[hi] - prefix[lo]) / (hi - lo) as f64;
            (v[i] as f64 - mean).max(0.0) as f32
        })
        .collect();
    let mean = out.iter().map(|&x| x as f64).sum::<f64>() / n.max(1) as f64;
    let var = out.iter().map(|&x| (x as f64 - mean).powi(2)).sum::<f64>() / n.max(1) as f64;
    let sd = var.sqrt().max(1e-9) as f32;
    for x in &mut out {
        *x /= sd;
    }
    out
}

/// Beat period in frames (fractional), from the envelope's autocorrelation
/// weighted by a log-normal prior around 120 BPM (one octave wide).
fn tempo_period(env: &[f32], fr: f64) -> f64 {
    let min_lag = (fr * 60.0 / 220.0).floor().max(1.0) as usize;
    let max_lag = ((fr * 60.0 / 55.0).ceil() as usize).min(env.len().saturating_sub(1));
    if max_lag <= min_lag + 2 {
        return fr * 0.5;
    }
    // Onset peaks are a frame wide and a beat is rarely a whole number of
    // frames (120 BPM = 21.5), so at the true lag only every other pair of
    // peaks would line up and twice the period would win. Smear them first.
    let k = [1.0f32, 2.0, 3.0, 2.0, 1.0];
    let env: Vec<f32> = (0..env.len())
        .map(|i| {
            let mut s = 0f32;
            for (j, w) in k.iter().enumerate() {
                let t = i as isize + j as isize - 2;
                if t >= 0 && (t as usize) < env.len() {
                    s += env[t as usize] * w;
                }
            }
            s / 9.0
        })
        .collect();
    let ac: Vec<f64> = (0..=max_lag + 1)
        .map(|lag| {
            if lag >= env.len() {
                return 0.0;
            }
            env[..env.len() - lag].iter().zip(&env[lag..]).map(|(a, b)| (*a * *b) as f64).sum::<f64>() / (env.len() - lag) as f64
        })
        .collect();
    let weight = |lag: f64| {
        let bpm = 60.0 * fr / lag;
        (-0.5 * ((bpm / 120.0).log2() / 1.0).powi(2)).exp()
    };
    let mut best = min_lag;
    let mut best_v = f64::MIN;
    for (lag, a) in ac.iter().enumerate().take(max_lag + 1).skip(min_lag) {
        let v = a * weight(lag as f64);
        if v > best_v {
            best_v = v;
            best = lag;
        }
    }
    // Parabolic refinement between neighbouring lags.
    let (a, b, c) = (ac[best - 1], ac[best], ac[best + 1]);
    let denom = a - 2.0 * b + c;
    let shift = if denom.abs() > 1e-12 { (0.5 * (a - c) / denom).clamp(-0.5, 0.5) } else { 0.0 };
    best as f64 + shift
}

/// Ellis' dynamic-programming beat tracker. Returns beat frames.
fn track(env: &[f32], period: f64) -> Vec<usize> {
    let n = env.len();
    if n == 0 || period < 1.0 {
        return Vec::new();
    }
    // Local score: the envelope smoothed by a Gaussian a 32nd of a beat wide.
    let half = period.round() as isize;
    let kernel: Vec<f32> = (-half..=half).map(|i| (-0.5 * (i as f64 * 32.0 / period).powi(2)).exp() as f32).collect();
    let local: Vec<f32> = (0..n as isize)
        .map(|t| {
            let mut s = 0f32;
            for (k, w) in kernel.iter().enumerate() {
                let j = t + k as isize - half;
                if j >= 0 && (j as usize) < n {
                    s += env[j as usize] * w;
                }
            }
            s
        })
        .collect();
    let tightness = 100.0;
    let lo = (period / 2.0).round() as usize;
    let hi = (2.0 * period).round() as usize;
    let mut cum = vec![0f64; n];
    let mut back: Vec<Option<usize>> = vec![None; n];
    for t in 0..n {
        let mut best: Option<(f64, usize)> = None;
        if t >= lo {
            let start = t.saturating_sub(hi);
            for (p, c) in cum.iter().enumerate().take(t - lo + 1).skip(start) {
                let d = (t - p) as f64;
                let pen = tightness * (d / period).ln().powi(2);
                let v = c - pen;
                if best.is_none_or(|(bv, _)| v > bv) {
                    best = Some((v, p));
                }
            }
        }
        match best {
            Some((v, p)) if v > 0.0 => {
                cum[t] = local[t] as f64 + v;
                back[t] = Some(p);
            }
            _ => cum[t] = local[t] as f64,
        }
    }
    // The last beat: the best cumulative score among the local maxima in the
    // final stretch (librosa takes the last peak above half the median).
    let mut peaks: Vec<usize> = (1..n.saturating_sub(1)).filter(|&t| cum[t] >= cum[t - 1] && cum[t] >= cum[t + 1]).collect();
    if peaks.is_empty() {
        return Vec::new();
    }
    let mut sorted: Vec<f64> = peaks.iter().map(|&t| cum[t]).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = sorted[sorted.len() / 2];
    peaks.retain(|&t| cum[t] >= 0.5 * median);
    let mut t = match peaks.last() {
        Some(&t) => t,
        None => return Vec::new(),
    };
    let mut beats = vec![t];
    while let Some(p) = back[t] {
        beats.push(p);
        t = p;
    }
    beats.reverse();
    // Trim weak beats at both ends (fade-ins, a silent tail).
    let rms = (local.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / n as f64).sqrt();
    let strong = |b: &usize| local[*b] as f64 >= 0.5 * rms;
    while beats.len() > 2 && !strong(&beats[0]) {
        beats.remove(0);
    }
    while beats.len() > 2 && !strong(beats.last().unwrap()) {
        beats.pop();
    }
    beats
}

/// The whole analysis over decoded samples.
pub fn analyze_samples(x: &[f32]) -> BeatInfo {
    let duration = x.len() as f64 / SR as f64;
    let fr = SR as f64 / HOP as f64;
    // Waveform outline: ~40 buckets a second, capped for long files.
    let buckets = ((duration * 40.0) as usize).clamp(1, 12_000);
    let per = (x.len() / buckets).max(1);
    let mut wave: Vec<f32> = x.chunks(per).map(|c| c.iter().fold(0f32, |m, s| m.max(s.abs()))).collect();
    let peak = wave.iter().cloned().fold(0f32, f32::max).max(1e-6);
    for w in &mut wave {
        *w = (*w / peak).min(1.0);
    }
    let wave_rate = wave.len() as f64 / duration.max(1e-6);

    let (env, bass) = onset_envelopes(x);
    if env.len() < 16 {
        return BeatInfo { duration, wave, wave_rate, ..Default::default() };
    }
    let period = tempo_period(&env, fr);
    let frames = track(&env, period);
    // Frame f's flux compares frame f with f-1: the onset is at f's centre.
    let to_s = |f: usize| ((f * HOP + N / 2) as f64 / SR as f64).min(duration);
    let beats: Vec<f64> = frames.iter().map(|&f| to_s(f)).collect();
    // Downbeats: the beat phase (of four) with the most bass (and some of
    // everything, for songs without much bottom end).
    let strength = |f: usize| {
        let w = 2;
        let lo = f.saturating_sub(w);
        let hi = (f + w + 1).min(env.len());
        (lo..hi).map(|i| bass[i] as f64 + 0.35 * env[i] as f64).fold(0f64, f64::max)
    };
    let mut best_phase = 0;
    let mut best = f64::MIN;
    for phase in 0..4 {
        let idx: Vec<usize> = (phase..frames.len()).step_by(4).collect();
        if idx.is_empty() {
            continue;
        }
        let s = idx.iter().map(|&i| strength(frames[i])).sum::<f64>() / idx.len() as f64;
        if s > best {
            best = s;
            best_phase = phase;
        }
    }
    let major: Vec<f64> = beats.iter().enumerate().filter(|(i, _)| *i % 4 == best_phase % 4).map(|(_, &b)| b).collect();
    let bpm = if beats.len() > 2 {
        let span = beats[beats.len() - 1] - beats[0];
        60.0 * (beats.len() - 1) as f64 / span.max(1e-6)
    } else {
        60.0 * fr / period
    };
    BeatInfo { duration, bpm: (bpm * 10.0).round() / 10.0, beats, major, wave, wave_rate }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A drum-machine bar: a kick (low thump) on beat 1, a click on the
    /// others, a little noise throughout, `bpm` for `secs`.
    fn click_track(bpm: f64, secs: f64, offset: f64) -> Vec<f32> {
        let n = (secs * SR as f64) as usize;
        let mut x = vec![0f32; n];
        let mut seed = 12345u32;
        for s in x.iter_mut() {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            *s = ((seed >> 16) as f32 / 65536.0 - 0.5) * 0.02;
        }
        let period = 60.0 / bpm;
        let mut k = 0;
        loop {
            let t = offset + k as f64 * period;
            if t >= secs - 0.3 {
                break;
            }
            let start = (t * SR as f64) as usize;
            let downbeat = k % 4 == 0;
            for i in 0..(0.12 * SR as f64) as usize {
                if start + i >= n {
                    break;
                }
                let tt = i as f64 / SR as f64;
                let env = (-tt * 35.0).exp();
                let v = if downbeat {
                    // kick: 60 Hz sine, loud
                    0.9 * (2.0 * std::f64::consts::PI * 60.0 * tt).sin() * env
                } else {
                    // click: 2.5 kHz, quieter
                    0.35 * (2.0 * std::f64::consts::PI * 2500.0 * tt).sin() * (-tt * 80.0).exp()
                };
                x[start + i] += v as f32;
            }
            k += 1;
        }
        x
    }

    fn check(bpm: f64, offset: f64) {
        let info = analyze_samples(&click_track(bpm, 30.0, offset));
        let period = 60.0 / bpm;
        assert!((info.bpm - bpm).abs() < 2.0, "bpm {} for {bpm}", info.bpm);
        assert!(info.beats.len() as f64 > 30.0 / period * 0.8, "only {} beats", info.beats.len());
        for w in info.beats.windows(2) {
            assert!(((w[1] - w[0]) - period).abs() < 0.04, "gap {} at {bpm} bpm", w[1] - w[0]);
        }
        // Every beat sits on a click (within 40 ms).
        for b in &info.beats {
            let k = ((b - offset) / period).round();
            assert!((b - (offset + k * period)).abs() < 0.04, "beat {b} off the grid");
        }
        // The downbeats are the kicks.
        assert!(info.major.len() >= info.beats.len() / 4 - 1);
        for m in &info.major {
            let k = ((m - offset) / period).round() as i64;
            assert_eq!(k % 4, 0, "major beat {m} isn't a downbeat ({bpm} bpm)");
        }
    }

    #[test]
    fn finds_beats_and_downbeats() {
        check(120.0, 0.25);
        check(92.0, 0.6);
        check(140.0, 0.1);
    }

    #[test]
    fn fft_matches_a_sine() {
        let mut re: Vec<f32> = (0..64).map(|i| (2.0 * std::f32::consts::PI * 4.0 * i as f32 / 64.0).cos()).collect();
        let mut im = vec![0f32; 64];
        fft(&mut re, &mut im);
        let mag: Vec<f32> = re.iter().zip(&im).map(|(a, b)| (a * a + b * b).sqrt()).collect();
        let peak = mag[..32].iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).unwrap().0;
        assert_eq!(peak, 4);
    }

    #[test]
    fn silence_has_no_beats() {
        let info = analyze_samples(&vec![0f32; SR as usize * 5]);
        assert!(info.beats.len() <= 2);
        assert!((info.duration - 5.0).abs() < 0.01);
    }
}
