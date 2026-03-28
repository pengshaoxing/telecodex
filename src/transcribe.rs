use std::path::{Path, PathBuf};

#[cfg(feature = "local-transcription")]
use std::fs;

use anyhow::{Context, Result, bail};

#[cfg(feature = "local-transcription")]
use tokio::process::Command;

#[cfg(feature = "local-transcription")]
use transcribe_rs::{
    SpeechModel, TranscribeOptions,
    onnx::{Quantization, parakeet::ParakeetModel},
};

#[cfg(feature = "local-transcription")]
use uuid::Uuid;

use crate::models::AttachmentTranscript;

#[cfg(feature = "local-transcription")]
const HANDY_MODEL_DIR_NAME: &str = "parakeet-tdt-0.6b-v3-int8";

pub fn detect_handy_parakeet_model_dir() -> Option<PathBuf> {
    #[cfg(not(feature = "local-transcription"))]
    return None;

    #[cfg(feature = "local-transcription")]
    handy_model_roots()
        .into_iter()
        .map(|root| root.join(HANDY_MODEL_DIR_NAME))
        .find(|candidate| is_valid_parakeet_model_dir(candidate))
}

pub async fn transcribe_audio_file(
    #[cfg_attr(not(feature = "local-transcription"), allow(unused_variables))]
    model_dir: PathBuf,
    #[cfg_attr(not(feature = "local-transcription"), allow(unused_variables))]
    source_path: PathBuf,
    #[cfg_attr(not(feature = "local-transcription"), allow(unused_variables))]
    scratch_dir: PathBuf,
) -> Result<AttachmentTranscript> {
    #[cfg(not(feature = "local-transcription"))]
    anyhow::bail!("local transcription not available (build with --features local-transcription)");

    #[cfg(feature = "local-transcription")]
    {
        let wav_path = scratch_dir.join(format!("{}.wav", Uuid::now_v7()));
        convert_audio_to_wav(&source_path, &wav_path).await?;

        let wav_for_transcription = wav_path.clone();
        let transcript_result =
            tokio::task::spawn_blocking(move || -> Result<AttachmentTranscript> {
                let mut model = ParakeetModel::load(&model_dir, &Quantization::Int8)
                    .with_context(|| {
                        format!("failed to load Handy model from {}", model_dir.display())
                    })?;
                let result = model
                    .transcribe_file(&wav_for_transcription, &TranscribeOptions::default())
                    .with_context(|| {
                        format!("failed to transcribe {}", wav_for_transcription.display())
                    })?;
                let text = result.text.trim().to_string();
                if text.is_empty() {
                    bail!("transcript is empty");
                }
                Ok(AttachmentTranscript {
                    engine: "Handy Parakeet".to_string(),
                    text,
                })
            })
            .await
            .context("audio transcription task join failed")?;

        let _ = fs::remove_file(&wav_path);
        transcript_result
    }
}

#[cfg(feature = "local-transcription")]
async fn convert_audio_to_wav(source_path: &Path, wav_path: &Path) -> Result<()> {
    let output = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(source_path)
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg("16000")
        .arg("-c:a")
        .arg("pcm_s16le")
        .arg(wav_path)
        .output()
        .await
        .with_context(|| format!("failed to spawn ffmpeg for {}", source_path.display()))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        bail!("ffmpeg exited with status {}", output.status);
    }
    bail!("ffmpeg exited with status {}: {stderr}", output.status);
}

#[cfg(feature = "local-transcription")]
fn handy_model_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(
            PathBuf::from(&appdata)
                .join("com.pais.handy")
                .join("models"),
        );
    }
    if let Some(local_appdata) = std::env::var_os("LOCALAPPDATA") {
        roots.push(
            PathBuf::from(local_appdata)
                .join("Handy")
                .join("resources")
                .join("models"),
        );
    }
    // macOS
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(
            PathBuf::from(&home)
                .join("Library/Application Support/com.pais.handy/models"),
        );
    }
    // Linux (XDG)
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        roots.push(PathBuf::from(data_home).join("com.pais.handy/models"));
    } else if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".local/share/com.pais.handy/models"));
    }
    roots
}

#[cfg(feature = "local-transcription")]
fn is_valid_parakeet_model_dir(dir: &Path) -> bool {
    [
        "encoder-model.int8.onnx",
        "decoder_joint-model.int8.onnx",
        "nemo128.onnx",
        "vocab.txt",
    ]
    .iter()
    .all(|name| dir.join(name).is_file())
}

async fn try_speedup_audio(source_path: &Path, speed_factor: f64) -> Option<PathBuf> {
    if speed_factor <= 1.0 {
        return None;
    }

    let extension = source_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("ogg");
    let sped_up_path = source_path.with_extension(format!("speedup.{extension}"));

    // FFmpeg atempo filter only supports 0.5..100.0; chain filters for values > 2.0
    let mut atempo_filters = Vec::new();
    let mut remaining = speed_factor;
    while remaining > 2.0 {
        atempo_filters.push("atempo=2.0".to_string());
        remaining /= 2.0;
    }
    atempo_filters.push(format!("atempo={remaining:.4}"));
    let filter_chain = atempo_filters.join(",");

    tracing::debug!(
        "ffmpeg: speeding up audio {:.1}x with filter '{filter_chain}'",
        speed_factor,
    );

    let result = tokio::process::Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(source_path)
        .arg("-filter:a")
        .arg(&filter_chain)
        .arg(&sped_up_path)
        .output()
        .await;

    match result {
        Ok(output) if output.status.success() => {
            tracing::info!(
                "ffmpeg: audio sped up {:.1}x, output={}",
                speed_factor,
                sped_up_path.display(),
            );
            Some(sped_up_path)
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(
                "ffmpeg speedup failed (status {}), falling back to original audio: {}",
                output.status,
                stderr.lines().last().unwrap_or_default(),
            );
            let _ = tokio::fs::remove_file(&sped_up_path).await;
            None
        }
        Err(err) => {
            tracing::warn!("ffmpeg not available for speedup, falling back to original audio: {err}");
            None
        }
    }
}

pub async fn transcribe_audio_remote(
    client: &reqwest::Client,
    config: &crate::config::WhisperConfig,
    source_path: &Path,
) -> Result<AttachmentTranscript> {
    let sped_up_file = try_speedup_audio(source_path, config.speed_factor).await;
    let effective_path = sped_up_file.as_deref().unwrap_or(source_path);
    let result = whisper_api_call(client, config, effective_path).await;
    // Always clean up the temporary sped-up file, regardless of success or failure.
    if let Some(path) = &sped_up_file {
        let _ = tokio::fs::remove_file(path).await;
    }
    result
}

async fn whisper_api_call(
    client: &reqwest::Client,
    config: &crate::config::WhisperConfig,
    audio_path: &Path,
) -> Result<AttachmentTranscript> {
    let api_key = config
        .resolve_api_key()
        .ok_or_else(|| anyhow::anyhow!("whisper API key not configured"))?;

    let file_name = audio_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.ogg")
        .to_string();
    let file_bytes = tokio::fs::read(audio_path)
        .await
        .with_context(|| format!("failed to read audio file: {}", audio_path.display()))?;

    let file_part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(file_name)
        .mime_str("application/octet-stream")?;

    let mut form = reqwest::multipart::Form::new()
        .part("file", file_part)
        .text("model", config.model.clone());

    if let Some(language) = &config.language {
        form = form.text("language", language.clone());
    }
    if let Some(prompt) = &config.initial_prompt {
        form = form.text("prompt", prompt.clone());
    }

    let url = format!(
        "{}/audio/transcriptions",
        config.api_base.trim_end_matches('/')
    );
    tracing::info!("whisper API request: POST {url} model={}", config.model);

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .multipart(form)
        .timeout(std::time::Duration::from_secs(config.timeout_seconds))
        .send()
        .await
        .with_context(|| format!("whisper API request failed: {url}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "empty response".to_string());
        bail!("whisper API returned {status}: {body}");
    }

    let body: serde_json::Value = response
        .json()
        .await
        .context("failed to parse whisper API response")?;

    let text = body
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();

    if text.is_empty() {
        bail!("whisper API returned empty transcript");
    }

    tracing::info!("whisper transcription complete: {}chars", text.len());

    Ok(AttachmentTranscript {
        engine: format!("Whisper ({})", config.model),
        text,
    })
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "local-transcription")]
    use super::*;

    #[cfg(feature = "local-transcription")]
    #[test]
    fn rejects_incomplete_model_dir() {
        let temp = tempfile::tempdir().unwrap();
        assert!(!is_valid_parakeet_model_dir(temp.path()));
    }
}
