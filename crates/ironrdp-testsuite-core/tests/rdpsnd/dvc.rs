use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use ironrdp_core::{decode, encode_vec};
use ironrdp_dvc::{DvcMessage, DvcProcessor as _};
use ironrdp_rdpsnd::client::{RdpsndClientHandler, RdpsndDvc};
use ironrdp_rdpsnd::pdu::{self, AudioFormat, PitchPdu, VolumePdu, WaveFormat};

fn pcm() -> AudioFormat {
    AudioFormat {
        format: WaveFormat::PCM,
        n_channels: 2,
        n_samples_per_sec: 44100,
        n_avg_bytes_per_sec: 176_400,
        n_block_align: 4,
        bits_per_sample: 16,
        data: None,
    }
}

/// Offers PCM and records the wave blocks it's given.
#[derive(Debug)]
struct Waves {
    formats: [AudioFormat; 1],
    received: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl RdpsndClientHandler for Waves {
    fn get_formats(&self) -> &[AudioFormat] {
        &self.formats
    }

    fn wave(&mut self, _format: &AudioFormat, _ts: u32, data: Cow<'_, [u8]>) {
        self.received.lock().unwrap().push(data.into_owned());
    }

    fn set_volume(&mut self, _volume: VolumePdu) {}

    fn set_pitch(&mut self, _pitch: PitchPdu) {}

    fn close(&mut self) {}
}

fn responses(messages: Vec<DvcMessage>) -> Vec<pdu::ClientAudioOutputPdu> {
    messages
        .iter()
        .map(|message| decode(&encode_vec(message.as_ref()).unwrap()).unwrap())
        .collect()
}

/// The whole exchange over the dynamic channel: formats, training, then a
/// wave, with the same replies the static channel sends and no static
/// channel header in front of them.
#[test]
fn plays_audio_over_the_dynamic_channel() {
    let waves = Arc::new(Mutex::new(Vec::new()));
    let mut client = RdpsndDvc::new(Box::new(Waves {
        formats: [pcm()],
        received: Arc::clone(&waves),
    }));
    assert_eq!(client.channel_name(), "AUDIO_PLAYBACK_DVC");
    assert!(client.start(7).unwrap().is_empty());

    let formats = encode_vec(&pdu::ServerAudioOutputPdu::AudioFormat(pdu::ServerAudioFormatPdu {
        version: pdu::Version::V8,
        formats: vec![pcm()],
    }))
    .unwrap();
    let replies = responses(client.process(7, &formats).unwrap());
    assert!(matches!(
        replies.as_slice(),
        [pdu::ClientAudioOutputPdu::AudioFormat(f), pdu::ClientAudioOutputPdu::QualityMode(_)] if f.formats == [pcm()]
    ));

    let training = encode_vec(&pdu::ServerAudioOutputPdu::Training(pdu::TrainingPdu {
        timestamp: 0x1234,
        data: vec![],
    }))
    .unwrap();
    let replies = responses(client.process(7, &training).unwrap());
    assert!(matches!(
        replies.as_slice(),
        [pdu::ClientAudioOutputPdu::TrainingConfirm(c)] if c.timestamp == 0x1234
    ));

    let wave = encode_vec(&pdu::ServerAudioOutputPdu::Wave2(pdu::Wave2Pdu {
        block_no: 3,
        format_no: 0,
        timestamp: 0x42,
        audio_timestamp: 0,
        data: Cow::Borrowed(&[1, 2, 3, 4]),
    }))
    .unwrap();
    let replies = responses(client.process(7, &wave).unwrap());
    assert!(matches!(
        replies.as_slice(),
        [pdu::ClientAudioOutputPdu::WaveConfirm(c)] if c.block_no == 3 && c.timestamp == 0x42
    ));
    assert_eq!(*waves.lock().unwrap(), [vec![1, 2, 3, 4]]);
}
