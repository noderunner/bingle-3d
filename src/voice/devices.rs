//! The microphone and the speaker, through cpal: each device at its own rate and channels, turned
//! into the call's mono 48 kHz on cpal's audio threads.

use std::sync::mpsc::Sender;

use bevy::log::info;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, Sample, SampleFormat, SizedSample, StreamConfig};

use super::Event;
use super::audio::{self, Capture, RATE, Resampler, SharedMixer};

/// Which device to use.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Choice {
    /// None: nothing captured, or nothing played.
    Off,
    /// The system's default.
    #[default]
    Default,
    /// The device with this id, as [`list`] gives them (cpal's, which lasts across restarts:
    /// names repeat, under ALSA); the default if it is not there.
    Device(String),
}

/// Open devices: sound flows while these are kept.
pub struct Streams {
    _input: Option<cpal::Stream>,
    _output: Option<cpal::Stream>,
}

/// A device a player can choose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
}

/// The microphones and the speakers.
pub fn list() -> (Vec<DeviceInfo>, Vec<DeviceInfo>) {
    let host = cpal::default_host();
    let infos = |devices: Option<Vec<Device>>| -> Vec<DeviceInfo> {
        devices
            .unwrap_or_default()
            .iter()
            .filter_map(info_of)
            .collect()
    };
    (
        infos(host.input_devices().ok().map(Iterator::collect)),
        infos(host.output_devices().ok().map(Iterator::collect)),
    )
}

fn info_of(d: &Device) -> Option<DeviceInfo> {
    Some(DeviceInfo {
        id: d.id().ok()?.to_string(),
        name: name_of(d),
    })
}

fn name_of(d: &Device) -> String {
    d.description()
        .map(|d| d.name().to_string())
        .unwrap_or_default()
}

/// Which of `devices` a choice means: `None` for the default (or a device that has gone).
pub fn pick(choice: &Choice, devices: &[DeviceInfo]) -> Option<usize> {
    match choice {
        Choice::Device(id) => devices.iter().position(|d| d.id == *id),
        _ => None,
    }
}

/// Opens the chosen devices, feeding `capture` and draining `mixer`. What will not open is said as
/// a [`Event::Problem`], and the call goes on without it.
pub fn open(
    microphone: &Choice,
    speaker: &Choice,
    capture: &Capture,
    mixer: &SharedMixer,
    events: &Sender<Event>,
) -> Streams {
    let host = cpal::default_host();
    let problem = |what: String| {
        let _ = events.send(Event::Problem(what));
    };
    let input = (*microphone != Choice::Off)
        .then(|| {
            let device = chosen(microphone, host.input_devices().ok(), || {
                host.default_input_device()
            })
            .ok_or_else(|| "No microphone was found.".to_owned())?;
            input(&device, capture.clone(), events.clone())
                .map_err(|e| format!("The microphone would not open: {e}"))
        })
        .and_then(|r| r.map_err(problem).ok());
    let output = (*speaker != Choice::Off)
        .then(|| {
            let device = chosen(speaker, host.output_devices().ok(), || {
                host.default_output_device()
            })
            .ok_or_else(|| "No speaker was found.".to_owned())?;
            output(&device, mixer.clone(), events.clone())
                .map_err(|e| format!("The speaker would not open: {e}"))
        })
        .and_then(|r| r.map_err(problem).ok());
    Streams {
        _input: input,
        _output: output,
    }
}

fn chosen(
    choice: &Choice,
    devices: Option<impl Iterator<Item = Device>>,
    default: impl FnOnce() -> Option<Device>,
) -> Option<Device> {
    let wanted = match choice {
        Choice::Device(id) => id,
        _ => return default(),
    };
    devices
        .and_then(|mut all| all.find(|d| d.id().is_ok_and(|id| id.to_string() == *wanted)))
        .or_else(default)
}

fn input(device: &Device, capture: Capture, events: Sender<Event>) -> Result<cpal::Stream, String> {
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let config: StreamConfig = supported.config();
    info!(
        "voice: the microphone is {} ({} Hz, {} channels)",
        name_of(device),
        config.sample_rate,
        config.channels
    );
    match supported.sample_format() {
        SampleFormat::F32 => input_as::<f32>(device, &config, capture, events),
        SampleFormat::I16 => input_as::<i16>(device, &config, capture, events),
        SampleFormat::U16 => input_as::<u16>(device, &config, capture, events),
        SampleFormat::I32 => input_as::<i32>(device, &config, capture, events),
        other => Err(format!("its samples are {other}, which this cannot read")),
    }
}

fn input_as<T>(
    device: &Device,
    config: &StreamConfig,
    capture: Capture,
    events: Sender<Event>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels.max(1));
    let mut resampler = Resampler::new(config.sample_rate, RATE);
    let (mut mono, mut out) = (Vec::new(), Vec::new());
    let stream = device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                mono.clear();
                mono.extend(data.chunks(channels).map(|frame| {
                    frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / channels as f32
                }));
                resampler.feed(&mono);
                out.clear();
                resampler.produce(&mut out, usize::MAX);
                capture.push(&out);
            },
            move |e| {
                let _ = events.send(Event::Problem(format!("The microphone stopped: {e}")));
            },
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn output(
    device: &Device,
    mixer: SharedMixer,
    events: Sender<Event>,
) -> Result<cpal::Stream, String> {
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let config: StreamConfig = supported.config();
    info!(
        "voice: the speaker is {} ({} Hz, {} channels)",
        name_of(device),
        config.sample_rate,
        config.channels
    );
    match supported.sample_format() {
        SampleFormat::F32 => output_as::<f32>(device, &config, mixer, events),
        SampleFormat::I16 => output_as::<i16>(device, &config, mixer, events),
        SampleFormat::U16 => output_as::<u16>(device, &config, mixer, events),
        SampleFormat::I32 => output_as::<i32>(device, &config, mixer, events),
        other => Err(format!("its samples are {other}, which this cannot write")),
    }
}

fn output_as<T>(
    device: &Device,
    config: &StreamConfig,
    mixer: SharedMixer,
    events: Sender<Event>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels.max(1));
    let mut resampler = Resampler::new(RATE, config.sample_rate);
    let (mut mixed, mut out) = (Vec::new(), Vec::new());
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                let frames = data.len() / channels;
                mixed.resize(resampler.wants(frames), 0.0);
                audio::lock(&mixer).pull(&mut mixed);
                resampler.feed(&mixed);
                out.clear();
                resampler.produce(&mut out, frames);
                for (i, frame) in data.chunks_mut(channels).enumerate() {
                    let s = T::from_sample(out.get(i).copied().unwrap_or(0.0));
                    frame.fill(s);
                }
            },
            move |e| {
                let _ = events.send(Event::Problem(format!("The speaker stopped: {e}")));
            },
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_is_picked_by_its_id_and_the_default_otherwise() {
        let device = |id: &str| DeviceInfo {
            id: id.into(),
            name: "HDA Intel PCH, ALC892 Analog".into(),
        };
        let devices = [
            device("alsa:front:CARD=PCH"),
            device("alsa:sysdefault:CARD=PCH"),
        ];
        let chosen = Choice::Device("alsa:sysdefault:CARD=PCH".into());
        assert_eq!(pick(&chosen, &devices), Some(1));
        assert_eq!(pick(&Choice::Device("alsa:gone".into()), &devices), None);
        assert_eq!(pick(&Choice::Default, &devices), None);
    }
}
