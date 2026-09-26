use crate::config::BotDetectionSettings;

pub struct BotDetectionResult {
    pub flags: Vec<String>,
    pub reject: bool,
}

/// Raw signal values from frame timing analysis, for storage/dashboards.
pub struct TimingSignals {
    pub variance_us2: f64,
    pub mean_offset_us: f64,
    pub client_claimed_secs: f64,
}

pub struct IpAnalysis {
    pub session_count: i64,
    pub account_count: i64,
}

/// Check if the frame count is consistent with server-measured wall-clock time.
/// This is unforgeable — the server controls both timestamps.
///
/// At 60fps, N frames should take N/60 seconds of real time.
/// We allow a tolerance window:
/// - Too fast (< 50% of expected): speedup or impossible
/// - Too slow (> 300% of expected): extreme slow-motion
pub fn analyze_server_timing(
    frames: u32,
    session_created_secs: i64,
    score_submitted_secs: i64,
) -> BotDetectionResult {
    let mut flags = Vec::new();
    let mut reject = false;

    if frames == 0 || session_created_secs >= score_submitted_secs {
        return BotDetectionResult { flags, reject };
    }

    let wall_clock_secs = (score_submitted_secs - session_created_secs) as f64;
    let expected_secs = frames as f64 / 60.0;

    // Ratio of actual time to expected time
    // 1.0 = perfect, <1.0 = faster than real-time, >1.0 = slower
    let ratio = wall_clock_secs / expected_secs;

    if ratio < 0.5 {
        // Played 2x faster than real-time — physically impossible without speedhack
        flags.push("impossible_speed".to_string());
        reject = true;
    } else if ratio > 5.0 && expected_secs > 10.0 {
        // 5x slower than real-time on a non-trivial game — extreme slow-motion
        // Only flag if the game was long enough to matter (>10s expected)
        flags.push("extreme_slow_motion".to_string());
    }

    BotDetectionResult { flags, reject }
}

pub fn analyze_ip_activity(
    analysis: &IpAnalysis,
    config: &BotDetectionSettings,
) -> BotDetectionResult {
    let mut flags = Vec::new();
    let mut reject = false;

    if analysis.account_count > config.max_accounts_per_ip_per_hour as i64 {
        flags.push("multi_account_ip".to_string());
        reject = true;
    }

    if analysis.session_count > config.max_sessions_per_ip_per_hour as i64 {
        flags.push("rapid_sessions".to_string());
        reject = true;
    }

    BotDetectionResult { flags, reject }
}

pub fn analyze_frame_timings(
    timing_bytes: &[u8],
    config: &BotDetectionSettings,
) -> BotDetectionResult {
    let mut flags = Vec::new();
    let reject = false;

    if timing_bytes.len() < 4 {
        // Not enough samples to analyze
        return BotDetectionResult { flags, reject };
    }

    // Decode i16 array (little-endian)
    let samples: Vec<i64> = timing_bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]) as i64)
        .collect();

    if samples.len() < 3 {
        return BotDetectionResult { flags, reject };
    }

    // Calculate mean offset
    let sum: i64 = samples.iter().sum();
    let mean = sum / samples.len() as i64;

    // Calculate variance
    let variance: u64 = samples
        .iter()
        .map(|&s| {
            let diff = s - mean;
            (diff * diff) as u64
        })
        .sum::<u64>()
        / samples.len() as u64;

    // Check 1: Low timing variance = likely bot (perfect setInterval)
    if variance < config.min_timing_variance_us2 {
        flags.push("low_timing_jitter".to_string());
    }

    // Check 2: High mean offset = slow-motion play
    if mean > config.max_mean_offset_us {
        flags.push("slow_motion".to_string());
    }

    // Check 3: Negative mean offset = speedup
    if mean < -config.max_mean_offset_us {
        flags.push("speedup".to_string());
    }

    BotDetectionResult { flags, reject }
}

/// Extract raw timing signals from frame timing bytes for storage/dashboards.
pub fn extract_timing_signals(timing_bytes: &[u8]) -> Option<TimingSignals> {
    if timing_bytes.len() < 4 {
        return None;
    }

    let samples: Vec<i64> = timing_bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]) as i64)
        .collect();

    if samples.is_empty() {
        return None;
    }

    let sum: i64 = samples.iter().sum();
    let mean = sum as f64 / samples.len() as f64;

    let variance: f64 = samples
        .iter()
        .map(|&s| {
            let diff = s as f64 - mean;
            diff * diff
        })
        .sum::<f64>()
        / samples.len() as f64;

    let total_offset_us: i64 = samples.iter().sum();
    let client_claimed_secs = (samples.len() + 1) as f64 + (total_offset_us as f64 / 1_000_000.0);

    Some(TimingSignals {
        variance_us2: variance,
        mean_offset_us: mean,
        client_claimed_secs,
    })
}

/// Cross-reference client-reported frame timings with server-measured elapsed time.
/// The sum of timing samples gives the client's claimed total play duration.
/// If this diverges significantly from server-measured elapsed time, the client
/// is lying about their timing data.
///
/// `timing_bytes`: i16 array of microsecond offsets from expected 1-second intervals
/// `server_elapsed_secs`: actual wall-clock seconds between session creation and score submission
pub fn cross_reference_timings(
    timing_bytes: &[u8],
    server_elapsed_secs: f64,
) -> BotDetectionResult {
    let mut flags = Vec::new();
    let mut reject = false;

    if timing_bytes.len() < 4 || server_elapsed_secs <= 0.0 {
        return BotDetectionResult { flags, reject };
    }

    // Decode i16 offsets (microseconds from expected 1-second intervals)
    let samples: Vec<i64> = timing_bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]) as i64)
        .collect();

    if samples.is_empty() {
        return BotDetectionResult { flags, reject };
    }

    // Client's claimed total time:
    // N+1 intervals (N samples = N+1 one-second windows including the first unsampled one)
    // Each sample is offset from 1 second in microseconds
    let total_offset_us: i64 = samples.iter().sum();
    let client_claimed_secs = (samples.len() + 1) as f64 + (total_offset_us as f64 / 1_000_000.0);

    if client_claimed_secs <= 0.0 {
        return BotDetectionResult { flags, reject };
    }

    // Ratio: how much does client's claim diverge from server reality?
    let ratio = client_claimed_secs / server_elapsed_secs;

    // Client claims 60s but server saw 30s → ratio ~2.0 → faked timing to hide speedhack
    // Client claims 60s but server saw 300s → ratio ~0.2 → faked timing to hide slow-motion
    if ratio > 1.5 {
        flags.push("timing_mismatch_speedhack".to_string());
        reject = true;
    } else if ratio < 0.5 {
        flags.push("timing_mismatch_slowmo".to_string());
        reject = true;
    }

    BotDetectionResult { flags, reject }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> BotDetectionSettings {
        BotDetectionSettings::default()
    }

    #[test]
    fn test_ip_analysis_clean() {
        let result = analyze_ip_activity(
            &IpAnalysis {
                session_count: 3,
                account_count: 1,
            },
            &default_config(),
        );
        assert!(result.flags.is_empty());
        assert!(!result.reject);
    }

    #[test]
    fn test_ip_analysis_multi_account() {
        let result = analyze_ip_activity(
            &IpAnalysis {
                session_count: 10,
                account_count: 8,
            },
            &default_config(),
        );
        assert!(result.flags.contains(&"multi_account_ip".to_string()));
        assert!(result.reject);
    }

    #[test]
    fn test_ip_analysis_rapid_sessions() {
        let result = analyze_ip_activity(
            &IpAnalysis {
                session_count: 25,
                account_count: 1,
            },
            &default_config(),
        );
        assert!(result.flags.contains(&"rapid_sessions".to_string()));
        assert!(result.reject);
    }

    #[test]
    fn test_timing_clean() {
        // Simulate human-like jitter: offsets scattered around 0 with ~10ms variance
        let samples: Vec<i16> = vec![
            5000, -3000, 8000, -7000, 2000, -4000, 6000, -1000, 9000, -5000,
        ];
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let result = analyze_frame_timings(&bytes, &default_config());
        assert!(!result.flags.contains(&"low_timing_jitter".to_string()));
        assert!(!result.flags.contains(&"slow_motion".to_string()));
    }

    #[test]
    fn test_timing_bot_low_jitter() {
        // Perfect timing: near-zero offsets
        let samples: Vec<i16> = vec![10, -5, 8, -3, 12, -7, 5, -10, 3, -2];
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let result = analyze_frame_timings(&bytes, &default_config());
        assert!(result.flags.contains(&"low_timing_jitter".to_string()));
    }

    #[test]
    fn test_timing_too_few_samples() {
        let result = analyze_frame_timings(&[0, 0], &default_config());
        assert!(result.flags.is_empty());
    }

    #[test]
    fn test_server_timing_normal() {
        // 3600 frames at 60fps = 60 seconds expected, submitted after 65 seconds
        let result = analyze_server_timing(3600, 1000, 1065);
        assert!(!result.reject);
        assert!(result.flags.is_empty());
    }

    #[test]
    fn test_server_timing_speedhack() {
        // 3600 frames = 60 seconds expected, but submitted after only 20 seconds
        // ratio = 20/60 = 0.33 — impossible
        let result = analyze_server_timing(3600, 1000, 1020);
        assert!(result.reject);
        assert!(result.flags.contains(&"impossible_speed".to_string()));
    }

    #[test]
    fn test_server_timing_extreme_slowmo() {
        // 3600 frames = 60 seconds expected, but submitted after 400 seconds
        // ratio = 400/60 = 6.67 — extreme slow motion
        let result = analyze_server_timing(3600, 1000, 1400);
        assert!(!result.reject); // flagged but not rejected
        assert!(result.flags.contains(&"extreme_slow_motion".to_string()));
    }

    #[test]
    fn test_cross_ref_consistent() {
        // 10 samples ≈ 11 seconds of client time, server saw 12 seconds — fine
        let samples: Vec<i16> = vec![0; 10];
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let result = cross_reference_timings(&bytes, 12.0);
        assert!(!result.reject);
        assert!(result.flags.is_empty());
    }

    #[test]
    fn test_cross_ref_faked_speedhack() {
        // Client claims ~11 seconds but server only saw 5 seconds
        let samples: Vec<i16> = vec![0; 10];
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let result = cross_reference_timings(&bytes, 5.0);
        assert!(result.reject);
        assert!(result
            .flags
            .contains(&"timing_mismatch_speedhack".to_string()));
    }

    #[test]
    fn test_cross_ref_faked_slowmo() {
        // Client claims ~11 seconds but server saw 60 seconds
        let samples: Vec<i16> = vec![0; 10];
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let result = cross_reference_timings(&bytes, 60.0);
        assert!(result.reject);
        assert!(result.flags.contains(&"timing_mismatch_slowmo".to_string()));
    }

    #[test]
    fn test_server_timing_short_game_ignored() {
        // Very short game (300 frames = 5 seconds) — don't flag slow motion
        // even if ratio is high, because the game is too short to matter
        let result = analyze_server_timing(300, 1000, 1100);
        assert!(!result.reject);
        assert!(result.flags.is_empty());
    }
}
