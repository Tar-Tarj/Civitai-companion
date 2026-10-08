#[cfg(target_os = "windows")]
const SOUND_1: &[u8] = include_bytes!("../resources/sounds/1.wav");
#[cfg(target_os = "windows")]
const SOUND_2: &[u8] = include_bytes!("../resources/sounds/2.wav");
#[cfg(target_os = "windows")]
const SOUND_3: &[u8] = include_bytes!("../resources/sounds/3.wav");
#[cfg(target_os = "windows")]
const SOUND_4: &[u8] = include_bytes!("../resources/sounds/4.wav");

#[cfg(target_os = "windows")]
pub fn play_notification_sound(sound_id: &str) -> bool {
    let bytes = match sound_id {
        "1" => SOUND_1,
        "2" => SOUND_2,
        "3" => SOUND_3,
        "4" => SOUND_4,
        _ => return false,
    };
    play_wave(bytes)
}

#[cfg(target_os = "windows")]
fn play_wave(bytes: &[u8]) -> bool {
    use windows::{
        Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT, SND_SYNC},
        core::PCWSTR,
    };

    unsafe {
        PlaySoundW(
            PCWSTR(bytes.as_ptr().cast()),
            None,
            SND_SYNC | SND_MEMORY | SND_NODEFAULT,
        )
        .as_bool()
    }
}

#[cfg(target_os = "windows")]
fn silent_warm_up_wave() -> Vec<u8> {
    const SAMPLE_RATE: u32 = 8_000;
    const SAMPLE_COUNT: u32 = 80;
    const DATA_BYTES: u32 = SAMPLE_COUNT * 2;
    let mut wave = Vec::with_capacity((44 + DATA_BYTES) as usize);
    wave.extend_from_slice(b"RIFF");
    wave.extend_from_slice(&(36 + DATA_BYTES).to_le_bytes());
    wave.extend_from_slice(b"WAVEfmt ");
    wave.extend_from_slice(&16_u32.to_le_bytes());
    wave.extend_from_slice(&1_u16.to_le_bytes());
    wave.extend_from_slice(&1_u16.to_le_bytes());
    wave.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wave.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    wave.extend_from_slice(&2_u16.to_le_bytes());
    wave.extend_from_slice(&16_u16.to_le_bytes());
    wave.extend_from_slice(b"data");
    wave.extend_from_slice(&DATA_BYTES.to_le_bytes());
    wave.resize((44 + DATA_BYTES) as usize, 0);
    wave
}

#[cfg(not(target_os = "windows"))]
pub fn play_notification_sound(_sound_id: &str) -> bool {
    false
}

pub async fn play_notification_sound_async(sound_id: String) -> bool {
    tokio::task::spawn_blocking(move || play_notification_sound(&sound_id))
        .await
        .unwrap_or(false)
}

pub async fn warm_up_notification_audio() -> bool {
    tokio::task::spawn_blocking(|| {
        #[cfg(target_os = "windows")]
        {
            return play_wave(&silent_warm_up_wave());
        }
        #[cfg(not(target_os = "windows"))]
        false
    })
    .await
    .unwrap_or(false)
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn embedded_notification_sounds_are_wave_files() {
        for sound in [SOUND_1, SOUND_2, SOUND_3, SOUND_4] {
            assert_eq!(&sound[..4], b"RIFF");
            assert_eq!(&sound[8..12], b"WAVE");
        }
    }

    #[test]
    fn warm_up_wave_is_short_silent_pcm() {
        let wave = silent_warm_up_wave();
        assert_eq!(&wave[..4], b"RIFF");
        assert_eq!(&wave[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(wave[40..44].try_into().unwrap()), 160);
        assert!(wave[44..].iter().all(|sample| *sample == 0));
    }

    #[test]
    fn unknown_sound_is_rejected_without_playback() {
        assert!(!play_notification_sound("invalid"));
    }
}
